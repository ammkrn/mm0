{-# LANGUAGE CPP #-}
{-# LANGUAGE FlexibleContexts #-}
{-# LANGUAGE LambdaCase #-}
{-# LANGUAGE MultiWayIf #-}
{-# LANGUAGE ScopedTypeVariables #-}
module MM0.Server (server) where

import Control.Concurrent
import Control.Concurrent.Async
import Control.Concurrent.STM
import qualified Control.Exception as E
import Control.Lens ((^.))
import Control.Monad.Reader
import Data.Default
import Data.Functor
import Data.Maybe
import qualified Data.HashMap.Strict as H
import qualified Data.Vector as V
import qualified Data.Text as T
import qualified Language.Haskell.LSP.Control as Ctrl (run)
import Language.Haskell.LSP.Core
import Language.Haskell.LSP.Diagnostics
import Language.Haskell.LSP.Messages
import Language.Haskell.LSP.Types hiding (ParseError)
import qualified Language.Haskell.LSP.Types.Lens as J
import Language.Haskell.LSP.VFS
import System.Timeout
import System.Exit
import qualified System.Log.Logger as L
import qualified Text.Megaparsec.Error as E
import qualified Data.Rope.UTF16 as Rope
import MM0.Compiler.PositionInfo
import qualified MM0.Compiler.AST as CA
import qualified MM0.Compiler.Parser as CP
import MM0.Compiler.PrettyPrinter hiding (doc)
import qualified MM0.Compiler.Env as CE
import qualified MM0.Compiler.Elaborator as CE
import MM0.Compiler.Elaborator (ErrorLevel(..))

server :: [String] -> IO ()
server ("--debug" : _) = atomically newTChan >>= run True
server _ = atomically newTChan >>= run False

catchAll :: forall a. IO a -> IO ()
catchAll m = () <$ (E.try m :: IO (Either E.SomeException a))

run :: Bool -> TChan FromClientMessage -> IO ()
run debug rin = do
  when debug $ catchAll $ setupLogger (Just "lsp.log") [] L.DEBUG
  exitCode <- Ctrl.run
    (InitializeCallbacks (const (Right ())) (const (Right ())) $
      \lf -> forkIO (reactor debug lf rin) >> return Nothing)
    lspHandlers
    lspOptions
    Nothing -- (Just "lsp-session.log")
  exitWith (if exitCode == 0 then ExitSuccess else ExitFailure exitCode)
  where
  lspOptions :: Options
  lspOptions = def {
    textDocumentSync = Just $ TextDocumentSyncOptions {
      _openClose         = Just True,
      _change            = Just TdSyncIncremental,
      _willSave          = Just False,
      _willSaveWaitUntil = Just False,
      _save              = Just $ SaveOptions $ Just False },
    executeCommandProvider = Just $ ExecuteCommandOptions $ List [] }

  lspHandlers :: Handlers
  lspHandlers = def {
    initializedHandler                       = Just $ passHandler NotInitialized,
    hoverHandler                             = Just $ passHandler ReqHover,
    definitionHandler                        = Just $ passHandler ReqDefinition,
    didOpenTextDocumentNotificationHandler   = Just $ passHandler NotDidOpenTextDocument,
    didChangeTextDocumentNotificationHandler = Just $ passHandler NotDidChangeTextDocument,
    didCloseTextDocumentNotificationHandler  = Just $ passHandler NotDidCloseTextDocument,
    didSaveTextDocumentNotificationHandler   = Just $ const $ return (),
    cancelNotificationHandler                = Just $ passHandler NotCancelRequestFromClient,
    responseHandler                          = Just $ passHandler RspFromClient }

  passHandler :: (a -> FromClientMessage) -> Handler a
  passHandler c msg = atomically $ writeTChan rin (c msg)


-- ---------------------------------------------------------------------

-- The reactor is a process that serialises and buffers all requests from the
-- LSP client, so they can be sent to the backend compiler one at a time, and a
-- reply sent.

data FileCache = FC {
  _fcText :: T.Text,
  _fcLines :: Lines,
  _fcAST :: CA.AST,
  _fcSpans :: V.Vector Spans,
  _fcEnv :: CE.Env }

data ReactorState = RS {
  rsDebug :: Bool,
  rsFuncs :: LspFuncs (),
  rsDiagThreads :: TVar (H.HashMap NormalizedUri (TextDocumentVersion, Async ())),
  rsLastParse :: TVar (H.HashMap NormalizedUri (TextDocumentVersion, FileCache)) }
type Reactor a = ReaderT ReactorState IO a

-- ---------------------------------------------------------------------
-- reactor monad functions
-- ---------------------------------------------------------------------

reactorSend :: FromServerMessage -> Reactor ()
reactorSend msg = do
  lf <- asks rsFuncs
  liftIO $ sendFunc lf msg

publishDiagnostics :: Int -> NormalizedUri -> TextDocumentVersion -> DiagnosticsBySource -> Reactor ()
publishDiagnostics maxToPublish uri v diags = do
  lf <- asks rsFuncs
  liftIO $ (publishDiagnosticsFunc lf) maxToPublish uri v diags

nextLspReqId :: Reactor LspId
nextLspReqId = asks (getNextReqId . rsFuncs) >>= liftIO

newDiagThread :: NormalizedUri -> TextDocumentVersion -> Reactor () -> Reactor ()
newDiagThread uri version m = ReaderT $ \rs -> do
  let dt = rsDiagThreads rs
  a <- async $ do
    timeout (10 * 1000000) (runReaderT m rs) >>= \case
      Just () -> return ()
      Nothing -> runReaderT (reactorErr "server timeout") rs
    as <- atomically $ do
      H.lookup uri <$> readTVar dt >>= \case
        Just (v', a) | not (isOutdated v' version) -> do
          modifyTVar dt $ H.delete uri
          return $ Just a
        _ -> return Nothing
    mapM_ cancel as
  old <- atomically $ do
    H.lookup uri <$> readTVar dt >>= \case
      Just (v', _) | isOutdated v' version -> return $ Just a
      Just (_, a') -> do
        modifyTVar dt $ H.insert uri (version, a)
        return $ Just a'
      Nothing -> return Nothing
  mapM_ cancel old

reactorSendId :: (LspId -> FromServerMessage) -> Reactor ()
reactorSendId msg = nextLspReqId >>= reactorSend . msg

reactorLogMsg :: MessageType -> T.Text -> Reactor ()
reactorLogMsg mt msg = reactorSend $ NotLogMessage $ fmServerLogMessageNotification mt msg

reactorErr :: T.Text -> Reactor ()
reactorErr = reactorLogMsg MtError

reactorLog :: T.Text -> Reactor ()
reactorLog s = do
  debug <- asks rsDebug
  when debug (reactorLogMsg MtLog s)

reactorLogs :: String -> Reactor ()
reactorLogs = reactorLog . T.pack

reactorHandle :: E.Exception e => (e -> Reactor ()) -> Reactor () -> Reactor ()
reactorHandle h m = ReaderT $ \lf ->
  E.handle (\e -> runReaderT (h e) lf) (runReaderT m lf)

reactorHandleAll :: Reactor () -> Reactor ()
reactorHandleAll = reactorHandle $ \(e :: E.SomeException) ->
  reactorErr $ T.pack $ E.displayException e

isOutdated :: Maybe Int -> Maybe Int -> Bool
isOutdated (Just n) (Just v) = v < n
isOutdated _ _ = False

-- ---------------------------------------------------------------------

-- | The single point that all events flow through, allowing management of state
-- to stitch replies and requests together from the two asynchronous sides: lsp
-- server and backend compiler
reactor :: Bool -> LspFuncs () -> TChan FromClientMessage -> IO ()
reactor debug lf inp = do
  th <- atomically $ newTVar H.empty
  fs <- atomically $ newTVar H.empty
  flip runReaderT (RS debug lf th fs) $ forever $ do
    liftIO (atomically $ readTChan inp) >>= \case
      -- Handle any response from a message originating at the server, such as
      -- "workspace/applyEdit"
      RspFromClient rm -> do
        reactorLogs $ "reactor:got RspFromClient:" ++ show rm

      NotInitialized _ -> do
        let registrations = []
        reactorSendId $ \n -> ReqRegisterCapability $ fmServerRegisterCapabilityRequest n $
          RegistrationParams $ List registrations

      NotDidOpenTextDocument msg -> do
        let TextDocumentItem uri _ version str = msg ^. J.params . J.textDocument
        sendDiagnostics (toNormalizedUri uri) (Just version) str

      NotDidChangeTextDocument msg -> do
        let VersionedTextDocumentIdentifier uri version = msg ^. J.params . J.textDocument
            doc = toNormalizedUri uri
        liftIO (getVirtualFileFunc lf doc) >>= \case
          Nothing -> reactorErr "reactor: Virtual File not found when processing DidChangeTextDocument"
          Just (VirtualFile _ str _) ->
            sendDiagnostics doc version (Rope.toText str)

      NotCancelRequestFromClient msg -> do
        reactorLogs $ "reactor:got NotCancelRequestFromClient:" ++ show msg

      ReqHover req -> do
        let TextDocumentPositionParams jdoc (Position l c) = req ^. J.params
            doc = toNormalizedUri $ jdoc ^. J.uri
        hover <- getFileCache doc <&> \case
          Just (FC _ larr ast sps env) -> do
            (stmt, CA.Span o1 pi' o2) <- getPosInfo ast sps (posToOff larr l c)
            makeHover env (toRange larr o1 o2) stmt pi'
          _ -> Nothing
        reactorSend $ RspHover $ makeResponseMessage req hover

      ReqDefinition req -> do
        let TextDocumentPositionParams jdoc (Position l c) = req ^. J.params
            uri = jdoc ^. J.uri
            doc = toNormalizedUri uri
            makeErr code msg = RspError $
              makeResponseError (responseId $ req ^. J.id) $
              ResponseError code msg Nothing
            makeResponse = RspDefinition . makeResponseMessage req
        resp <- getFileCache doc <&> \case
          Just (FC _ larr ast sps env) -> Just $ do
            (stmt, CA.Span _ pi' _) <- maybeToList $ getPosInfo ast sps (posToOff larr l c)
            goToDefinition larr env stmt pi'
          _ -> Nothing
        reactorSend $ case resp of
          Nothing -> makeErr InternalError "could not get file data"
          Just [a] -> makeResponse $ SingleLoc $ Location uri a
          Just as -> makeResponse $ MultiLoc $ Location uri <$> as

      om -> reactorLogs $ "reactor: got HandlerRequest:" ++ show om

-- ---------------------------------------------------------------------

elSeverity :: ErrorLevel -> DiagnosticSeverity
elSeverity ELError = DsError
elSeverity ELWarning = DsWarning
elSeverity ELInfo = DsInfo

mkDiagnosticRelated :: ErrorLevel -> Range -> T.Text ->
  [DiagnosticRelatedInformation] -> Diagnostic
mkDiagnosticRelated l r msg rel =
  Diagnostic
    r
    (Just (elSeverity l))  -- severity
    Nothing  -- code
    (Just "MM0") -- source
    msg
    (Just (List rel))

mkDiagnostic :: Position -> T.Text -> Diagnostic
mkDiagnostic p msg = mkDiagnosticRelated ELError (Range p p) msg []

parseErrorDiags :: Lines -> [CP.ParseError] -> [Diagnostic]
parseErrorDiags larr errs = toDiag <$> errs where
  toDiag err = let (l, c) = offToPos larr (E.errorOffset err) in
    mkDiagnostic (Position l c) (T.pack (E.parseErrorTextPretty err))

toPosition :: Lines -> Int -> Position
toPosition larr n = let (l, c) = offToPos larr n in Position l c

toRange :: Lines -> Int -> Int -> Range
toRange larr o1 o2 = Range (toPosition larr o1) (toPosition larr o2)

elabErrorDiags :: Uri -> Lines -> [CE.ElabError] -> [Diagnostic]
elabErrorDiags uri larr errs = toDiag <$> errs where
  toRel :: (Int, Int, T.Text) -> DiagnosticRelatedInformation
  toRel (o1, o2, msg) = DiagnosticRelatedInformation
    (Location uri (toRange larr o1 o2)) msg
  toDiag :: CE.ElabError -> Diagnostic
  toDiag (CE.ElabError l o1 o2 msg es) =
    mkDiagnosticRelated l (toRange larr o1 o2) msg (toRel <$> es)

-- | Analyze the file and send any diagnostics to the client in a
-- "textDocument/publishDiagnostics" msg
sendDiagnostics :: NormalizedUri -> TextDocumentVersion -> T.Text -> Reactor ()
sendDiagnostics fileNUri@(NormalizedUri t) version str = do
  fs <- asks rsLastParse
  liftIO (readTVarIO fs) >>= \m -> case H.lookup fileNUri m of
    Just (oldv, _) | isOutdated oldv version -> return ()
    _ -> reactorHandleAll $ newDiagThread fileNUri version $ do
      let fileUri = fromNormalizedUri fileNUri
          file = fromMaybe "" $ uriToFilePath fileUri
          larr = getLines str
          isMM0 = T.isSuffixOf "mm0" t
      diags <- case CP.parseAST file str of
        (errs, _, Nothing) -> return $ parseErrorDiags larr errs
        (errs, _, Just ast) -> do
          (errs', env) <- liftIO $ CE.elaborate isMM0 errs ast
          liftIO $ atomically $ modifyTVar fs $ flip H.alter fileNUri $ \case
            fc@(Just (oldv, _)) | isOutdated oldv version -> fc
            _ -> Just (version, FC str larr ast (toSpans env <$> ast) env)
          return (elabErrorDiags fileUri larr errs')
      publishDiagnostics 100 fileNUri version (partitionBySource diags)

getFileCache :: NormalizedUri -> Reactor (Maybe FileCache)
getFileCache doc = do
  lf <- asks rsFuncs
  liftIO (getVirtualFileFunc lf doc) >>= \case
    Nothing -> return Nothing
    Just (VirtualFile version str _) ->
      let
        tryGet 0 = return Nothing
        tryGet retries = do
          sendDiagnostics doc (Just version) (Rope.toText str)
          dt <- asks rsDiagThreads
          liftIO $ do
            a <- atomically (H.lookup doc <$> readTVar dt)
            mapM_ (wait . snd) a
          go (retries - 1)
        go retries = do
          m <- asks rsLastParse >>= liftIO . readTVarIO
          case H.lookup doc m of
            Just (Just oldv, fc) | oldv == version -> return (Just fc)
            Just (oldv, _) | isOutdated oldv (Just version) -> tryGet retries
            Nothing -> tryGet retries
            _ -> return Nothing
      in go (1 :: Int)

makeHover :: CE.Env -> Range -> CA.AtPos CA.Stmt -> PosInfo -> Maybe Hover
makeHover env range stmt (PosInfo t pi') = case pi' of
  PISort -> do
    (_, o, sd) <- H.lookup t (CE.eSorts env)
    Just $ code $ ppStmt $ CA.Sort o t sd
  PIVar (Just bi) -> Just $ code $ ppBinder bi
  PIVar Nothing -> do
    CA.AtPos _ (CA.Decl _ _ _ st _ _ _) <- return stmt
    bis <- H.lookup st (CE.eDecls env) <&> \case
      (_, _, CE.DTerm bis _, _) -> bis
      (_, _, CE.DAxiom bis _ _, _) -> bis
      (_, _, CE.DDef _ bis _ _, _) -> bis
      (_, _, CE.DTheorem _ bis _ _ _, _) -> bis
    bi:_ <- return $ filter (\bi -> CE.binderName bi == t) bis
    Just $ code $ ppPBinder bi
  PITerm -> do
    (_, _, d, _) <- H.lookup t (CE.eDecls env)
    Just $ code $ ppDecl env t d
  PIAtom True (Just bi) -> Just $ code $ ppBinder bi
  PIAtom True Nothing -> do
    (_, _, d, _) <- H.lookup t (CE.eDecls env)
    Just $ code $ ppDecl env t d
  _ -> Nothing
  where

  hover ms = Hover (HoverContents ms) (Just range)
  code = hover . markedUpContent "mm0" . render'

goToDefinition :: Lines -> CE.Env -> CA.AtPos CA.Stmt -> PosInfo -> [Range]
goToDefinition larr env _ (PosInfo t pi') = case pi' of
  PISort -> maybeToList $
    H.lookup t (CE.eSorts env) <&> \(_, o, _) -> range o t
  PIVar bi -> maybeToList $ binderRange <$> bi
  PITerm -> maybeToList $
    H.lookup t (CE.eDecls env) <&> \(_, o, _, _) -> range o t
  PIAtom b obi ->
    (case (b, obi) of
      (True, Just bi) -> [binderRange bi]
      (True, Nothing) -> maybeToList $
        H.lookup t (CE.eDecls env) <&> \(_, o, _, _) -> range o t
      _ -> []) ++
    maybeToList (
      H.lookup t (CE.eLispNames env) >>= fst <&> \o -> range o t)
  where
  range o t' = toRange larr o (o + T.length t')
  binderRange (CA.Binder o l _) =
    range o (fromMaybe "_" (CA.localName l))
