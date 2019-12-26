pub mod environment;
pub mod lisp;
pub mod math_parser;
pub mod local_context;
pub mod tactic;
pub mod proof;

use std::ops::{Deref, DerefMut};
use std::sync::Arc;
use std::time::{Instant, Duration};
use std::path::PathBuf;
use std::collections::{HashMap};
use lsp_types::{Diagnostic, DiagnosticRelatedInformation};
use environment::*;
use environment::Literal as ELiteral;
use lisp::{LispVal, LispKind, FALSE};
pub use {environment::Environment, local_context::LocalContext};
pub use crate::parser::ErrorLevel;
use crate::util::*;
use crate::parser::{*, ast::*, ast::Literal as ALiteral};
use crate::lined_string::*;

#[derive(Debug)]
pub enum ElabErrorKind {
  Boxed(BoxError, Option<Vec<(FileSpan, BoxError)>>),
}
impl ElabErrorKind {
  pub fn msg(&self) -> String {
    match self {
      ElabErrorKind::Boxed(e, _) => format!("{}", e),
    }
  }

  pub fn to_related_info(&self, file: &LinedString) -> Option<Vec<DiagnosticRelatedInformation>> {
    match self {
      ElabErrorKind::Boxed(_, Some(info)) =>
        Some(info.iter().map(|(fs, e)| DiagnosticRelatedInformation {
          location: file.to_loc(fs),
          message: format!("{}", e),
        }).collect()),
      _ => None
    }
  }
}

impl From<BoxError> for ElabErrorKind {
  fn from(e: BoxError) -> ElabErrorKind { ElabErrorKind::Boxed(e, None) }
}

#[derive(Debug)]
pub struct ElabError {
  pub pos: Span,
  pub level: ErrorLevel,
  pub kind: ElabErrorKind,
}
pub type Result<T> = std::result::Result<T, ElabError>;

impl ElabError {
  pub fn new(pos: impl Into<Span>, kind: ElabErrorKind) -> ElabError {
    ElabError { pos: pos.into(), level: ErrorLevel::Error, kind }
  }
  pub fn new_e(pos: impl Into<Span>, e: impl Into<BoxError>) -> ElabError {
    ElabError::new(pos, ElabErrorKind::Boxed(e.into(), None))
  }
  pub fn with_info(pos: impl Into<Span>, msg: BoxError, v: Vec<(FileSpan, BoxError)>) -> ElabError {
    ElabError::new(pos, ElabErrorKind::Boxed(msg, Some(v)))
  }

  pub fn info(pos: impl Into<Span>, e: impl Into<BoxError>) -> ElabError {
    ElabError { pos: pos.into(), level: ErrorLevel::Info, kind: ElabErrorKind::Boxed(e.into(), None)}
  }

  pub fn to_diag(&self, file: &LinedString) -> Diagnostic {
    Diagnostic {
      range: file.to_range(self.pos),
      severity: Some(self.level.to_diag_severity()),
      code: None,
      source: Some("mm0-rs".to_owned()),
      message: self.kind.msg(),
      related_information: self.kind.to_related_info(file),
    }
  }
}

impl From<ParseError> for ElabError {
  fn from(e: ParseError) -> Self {
    ElabError {pos: e.pos, level: e.level, kind: ElabErrorKind::Boxed(e.msg, None) }
  }
}

struct ReportMode {
  error: bool,
  warn: bool,
  info: bool,
}

impl ReportMode {
  fn new() -> ReportMode {
    ReportMode {error: true, warn: true, info: true}
  }

  fn active(&self, lvl: ErrorLevel) -> bool {
    match lvl {
      ErrorLevel::Error => self.error,
      ErrorLevel::Warning => self.warn,
      ErrorLevel::Info => self.info,
    }
  }
}

pub struct Elaborator<'a, F: FileServer + ?Sized> {
  ast: &'a AST,
  fs: &'a F,
  path: FileRef,
  errors: Vec<ElabError>,
  toks: HashMap<Span, Option<F::WaitToken>>,
  env: Environment,
  timeout: Option<Duration>,
  cur_timeout: Option<Instant>,
  lc: LocalContext,
  reporting: ReportMode,
}

impl<F: FileServer + ?Sized> Deref for Elaborator<'_, F> {
  type Target = Environment;
  fn deref(&self) -> &Environment { &self.env }
}
impl<F: FileServer + ?Sized> DerefMut for Elaborator<'_, F> {
  fn deref_mut(&mut self) -> &mut Environment { &mut self.env }
}

impl<'a, F: FileServer + ?Sized> Elaborator<'a, F> {
  fn new(ast: &'a AST, path: FileRef, fs: &'a F) -> Elaborator<'a, F> {
    Elaborator {
      ast, fs, path,
      errors: Vec::new(),
      toks: HashMap::new(),
      env: Environment::new(),
      timeout: Some(Duration::from_secs(5)),
      cur_timeout: None,
      lc: LocalContext::new(),
      reporting: ReportMode::new(),
    }
  }

  fn span(&self, s: Span) -> &str { self.ast.span(s) }
  fn fspan(&self, span: Span) -> FileSpan { FileSpan {file: self.path.clone(), span} }
  fn report(&mut self, e: ElabError) {
    if self.reporting.active(e.level) {self.errors.push(e)}
  }
  fn catch(&mut self, r: Result<()>) { r.unwrap_or_else(|e| self.report(e)) }

  fn name_of(&mut self, stmt: &Stmt) -> LispVal {
    match &stmt.k {
      StmtKind::Annot(_, s) => self.name_of(s),
      StmtKind::Decl(d) => Arc::new(LispKind::Atom(self.env.get_atom(self.ast.span(d.id)))),
      &StmtKind::Sort(id, _) => Arc::new(LispKind::Atom(self.env.get_atom(self.ast.span(id)))),
      _ => FALSE.clone(),
    }
  }

  fn elab_simple_nota(&mut self, n: &SimpleNota) -> Result<()> {
    let a = self.env.get_atom(self.ast.span(n.id));
    let term = self.term(a).ok_or_else(|| ElabError::new_e(n.id, "term not declared"))?;
    let tk: ArcString = self.span(n.c.trim).into();
    let (rassoc, nargs, lits) = match n.k {
      SimpleNotaKind::Prefix => (true, 1, vec![ELiteral::Var(0, n.prec)]),
      SimpleNotaKind::Infix {right} =>
        if let Prec::Prec(i) = n.prec {
          let i2 = i.checked_add(1).ok_or_else(|| ElabError::new_e(n.id, "precedence out of range"))?;
          let (l, r) = if right {(i2, i)} else {(i, i2)};
          (right, 2, vec![
            ELiteral::Var(0, Prec::Prec(l)),
            ELiteral::Const(tk.clone()),
            ELiteral::Var(1, Prec::Prec(r))])
        } else { Err(ElabError::new_e(n.id, "max prec not allowed for infix"))? }
    };
    self.check_term_nargs(n.id, term, nargs)?;
    self.add_const(n.c.trim, n.prec)?;
    let info = NotaInfo { span: self.fspan(n.id), term, nargs, rassoc: Some(rassoc), lits };
    match n.k {
      SimpleNotaKind::Prefix => self.pe.add_prefix(tk.clone(), info),
      SimpleNotaKind::Infix {..} => self.pe.add_infix(tk.clone(), info),
    }.map_err(|r| ElabError::with_info(n.id,
      format!("constant '{}' already declared", tk).into(),
      vec![(r.decl1, "declared here".into())]))
  }

  fn elab_coe(&mut self, id: Span, from: Span, to: Span) -> Result<()> {
    let aid = self.env.get_atom(self.ast.span(id));
    let afrom = self.env.get_atom(self.ast.span(from));
    let ato = self.env.get_atom(self.ast.span(to));
    let t = self.term(aid).ok_or_else(|| ElabError::new_e(id, "term not declared"))?;
    let s1 = self.data[afrom].sort.ok_or_else(|| ElabError::new_e(from, "sort not declared"))?;
    let s2 = self.data[ato].sort.ok_or_else(|| ElabError::new_e(to, "sort not declared"))?;
    let fsp = self.fspan(id);
    self.check_term_nargs(id, t, 2)?;
    self.add_coe(s1, s2, fsp, t)
  }

  fn add_const(&mut self, tk: Span, p: Prec) -> Result<()> {
    let s = self.span(tk).into();
    let fsp = self.fspan(tk);
    self.pe.add_const(s, fsp, p).map_err(|r| ElabError::with_info(tk,
      "constant already declared with a different precedence".into(),
      vec![(r.decl1, "declared here".into())]))
  }

  fn elab_gen_nota(&mut self, n: &GenNota) -> Result<()> {
    let a = self.env.get_atom(self.ast.span(n.id));
    let term = self.term(a).ok_or_else(|| ElabError::new_e(n.id, "term not declared"))?;
    let nargs = n.bis.len();
    self.check_term_nargs(n.id, term, nargs)?;
    let mut vars = HashMap::<&str, (usize, bool)>::new();
    for (i, bi) in n.bis.iter().enumerate() {
      match bi.kind {
        LocalKind::Dummy => Err(ElabError::new_e(bi.local.unwrap_or(bi.span),
          "dummies not permitted in notation declarations"))?,
        LocalKind::Anon => Err(ElabError::new_e(bi.local.unwrap_or(bi.span),
          "all variables must be used in notation declaration"))?,
        _ => { vars.insert(self.ast.span(bi.local.unwrap_or(bi.span)), (i, false)); }
      }
    }

    fn bump(yes: bool, sp: Span, p: Prec) -> Result<Prec> {
      if !yes {return Ok(p)}
      if let Prec::Prec(n) = p {
        if let Some(i) = n.checked_add(1) { Ok(Prec::Prec(i)) }
        else {Err(ElabError::new_e(sp, "precedence out of range"))}
      } else {Err(ElabError::new_e(sp, "infix constants cannot have prec max"))}
    }
    let mut get_var = |elab: &mut Self, sp: Span| -> Result<usize> {
      let v = vars.get_mut(elab.span(sp))
        .ok_or_else(|| ElabError::new_e(sp, "variable not found"))?;
      v.1 = true;
      Ok(v.0)
    };

    let mut it = n.lits.iter().peekable();
    let (mut lits, mut rassoc, infix, tk, prec) = match it.next() {
      None => Err(ElabError::new_e(n.id, "notation requires at least one literal"))?,
      Some(&ALiteral::Const(ref c, p)) => (vec![], None, false, c, p),
      Some(&ALiteral::Var(v)) => match it.next() {
        None => Err(ElabError::new_e(v, "notation requires at least one constant"))?,
        Some(&ALiteral::Var(v)) => Err(ElabError::new_e(v, "notation cannot start with two variables"))?,
        Some(&ALiteral::Const(ref c, p)) => {
          let r = match n.prec {
            None => None,
            Some((q, _)) if q != p => Err(ElabError::new_e(c.fmla.0, "notation precedence must match first constant"))?,
            Some((_, r)) => Some(r),
          };
          (vec![
            ELiteral::Var(get_var(self, v)?, bump(r.unwrap_or(false), c.fmla.0, p)?),
            ELiteral::Const(self.span(c.trim).into())],
          r, true, c, p)
        }
      }
    };

    self.add_const(tk.trim, prec)?;
    while let Some(lit) = it.next() {
      match lit {
        &ALiteral::Const(ref c, p) => {
          lits.push(ELiteral::Const(self.span(c.trim).into()));
          self.add_const(c.trim, p)?;
        }
        &ALiteral::Var(v) => {
          let prec = match it.peek() {
            None => {
              let r = if let Some(r) = rassoc {r} else {
                if let Some((_, r)) = n.prec {r} else {
                  Err(ElabError::new_e(n.id, "general infix notation requires explicit associativity"))?
                }
              };
              rassoc = Some(r);
              bump(!r, tk.fmla.0, prec)?
            }
            Some(&&ALiteral::Const(ref c, p)) => bump(true, c.fmla.0, p)?,
            Some(ALiteral::Var(_)) => Prec::Max,
          };
          lits.push(ELiteral::Var(get_var(self, v)?, prec));
        }
      }
    }

    for (_, (i, b)) in vars {
      if !b { Err(ElabError::new_e(n.bis[i].local.unwrap_or(n.bis[i].span), "variable not used in notation"))? }
    }
    let s: ArcString = self.span(tk.trim).into();
    let info = NotaInfo { span: self.fspan(n.id), term, nargs, rassoc, lits };
    match infix {
      false => self.pe.add_prefix(s.clone(), info),
      true => self.pe.add_infix(s.clone(), info),
    }.map_err(|r| ElabError::with_info(n.id,
      format!("constant '{}' already declared", s).into(),
      vec![(r.decl1, "declared here".into())]))
  }

  fn parse_and_print(&mut self, e: &SExpr) -> Result<()> {
    let val = self.eval_lisp(e)?;
    if val.is_def() {self.print_lisp(e.span, &val)}
    Ok(())
  }

  fn elab_stmt(&mut self, stmt: &Stmt) -> Result<()> {
    self.cur_timeout = self.timeout.and_then(|d| Instant::now().checked_add(d));
    match &stmt.k {
      &StmtKind::Sort(sp, sd) => {
        let a = self.env.get_atom(self.ast.span(sp));
        let fsp = self.fspan(sp);
        self.add_sort(a, fsp, sd).map_err(|e| e.to_elab_error(sp))?;
      }
      StmtKind::Decl(d) => self.elab_decl(stmt.span, d)?,
      StmtKind::Delimiter(Delimiter::Both(f)) => self.pe.add_delimiters(f, f),
      StmtKind::Delimiter(Delimiter::LeftRight(ls, rs)) => self.pe.add_delimiters(ls, rs),
      StmtKind::SimpleNota(n) => self.elab_simple_nota(n)?,
      &StmtKind::Coercion {id, from, to} => self.elab_coe(id, from, to)?,
      StmtKind::Notation(n) => self.elab_gen_nota(n)?,
      &StmtKind::Import(sp, _) => if let Some(tok) = &self.toks[&sp] {
        self.env.merge(&self.fs.get_elab(tok), sp, &mut self.errors)?
      },
      StmtKind::Do(es) => for e in es { self.parse_and_print(e)? },
      StmtKind::Annot(e, s) => {
        let v = self.eval_lisp(e)?;
        self.elab_stmt(s)?;
        let ann = self.get_atom("annotate");
        let ann = match &self.data[ann].lisp {
          Some((_, e)) => e.clone(),
          None => Err(ElabError::new_e(e.span, "define 'annotate' before using annotations"))?,
        };
        let args = vec![v, self.name_of(s)];
        self.call_func(e.span, ann, args)?;
      },
      _ => Err(ElabError::new_e(stmt.span, "unimplemented"))?
    }
    Ok(())
  }
}

pub trait FileServer {
  type WaitToken: Clone;
  fn request_elab(&self, path: PathBuf, f: impl Fn(BoxError) -> ElabError) ->
    Result<(FileRef, Self::WaitToken)>;

  fn get_elab(&self, tok: &Self::WaitToken) -> Arc<Environment>;

  fn elaborate<'a>(&'a self, path: FileRef, ast: &'a AST,
      _old: Option<(usize, Vec<ElabError>, Arc<Environment>)>) ->
      (Vec<ElabError>, Environment, Vec<FileRef>) {
    let mut elab = Elaborator::new(ast, path, self);
    let mut deps: Vec<FileRef> = Vec::new();
    for (sp, f) in &ast.imports {
      match elab.path.path().join(f).canonicalize()
        .map_err(|e| ElabError::new_e(sp.clone(), e))
        .and_then(|p| self.request_elab(p, |e| ElabError::new_e(sp.clone(), e))) {
        Ok((buf, tok)) => { deps.push(buf); elab.toks.insert(sp.clone(), Some(tok)); }
        Err(e) => { elab.report(e); elab.toks.insert(sp.clone(), None); }
      }
    }

    for s in ast.stmts.iter() {
      let r = elab.elab_stmt(s);
      elab.catch(r)
    }
    (elab.errors, elab.env, deps)
  }
}