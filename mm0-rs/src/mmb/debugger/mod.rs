use std::borrow::Cow;
use std::fmt::Write;
use serde_json::{ json, Value };
use bumpalo::Bump;
use bumpalo::collections::Vec as BumpVec;
use mm0_util::ids::TermId;
use mmb_parser::ty::{ Arg, Type };
use mmb_parser::{ MmbFile, NumdStmtCmd, ProofCmd, UnifyCmd };
use mmb_parser::parser::{ ProofIter, TermRef, ThmRef, VarListRef };
use serde::Deserialize;
use crate::mmb::debugger::util::{ Res, VerifErr, Either };
use crate::mmb::debugger::proof::{ Mode, Finish };
use crate::mmb::debugger::unify::UMode;
use crate::elab::environment::{
    Environment,
    NotaInfo,
    Literal::*,
};

#[macro_export]
macro_rules! err_msg {
    ( $s:expr ) => {
        VerifErr::LocalMsg(file!(), line!(), $s)
    }
}

#[macro_export]
macro_rules! localize {
    ( $e:expr ) => {
        $e.map_err(|e| VerifErr::Local(file!(), line!(), Box::new(e)))
    }
}

#[macro_export]
macro_rules! none_err {
    ( $e:expr ) => {
        $e.ok_or(VerifErr::NoneErr(file!(), line!()))
    }
}

#[macro_export]
macro_rules! make_sure {
    ( $e:expr ) => {
        if !($e) {
            return Err(VerifErr::MakeSure(file!(), line!()))
        }
    }
}

mod proof;
mod unify;
mod util;


const STACK_OPEN: &'static str = r#"<li class="stack_item">"#;
const HEAP_OPEN: &'static str = r#"<li class="heap_item">"#;
const LI_CLOSE: &'static str = "</li>";
const VAR_OPEN: &'static str = r#"<span class="var">"#;
const BVAR_OPEN: &'static str = r#"<span class="bvar">"#;
const DUMMY_OPEN: &'static str = r#"<span class="dummy">"#;
const MMB_OPEN: &'static str = r#"<span class="mmb">"#;
const SPAN_CLOSE: &'static str = "</span>";
const TSTYLE: &'static str = r#"<span class="mmb">|-</mmb>"#;
const CONV: &'static str = r#"<span class="mmb"> == </mmb>"#;
const COCONV: &'static str = r#"<span class="mmb"> =?= </mmb>"#;

// Each sort has one byte associated to it, which
// contains flags for the sort modifiers.
// The high four bits are unused.
pub(in crate::mmb::debugger) const SORT_STRICT   : u8 = 2;
pub(in crate::mmb::debugger) const SORT_PROVABLE : u8 = 4;

/// 10000000_00000000_00000000_00000000_00000000_00000000_00000000_00000000
pub(in crate::mmb::debugger) const TYPE_BOUND_MASK: u64 = 1 << 63;

/// 00000000_11111111_11111111_11111111_11111111_11111111_11111111_11111111
pub(in crate::mmb::debugger) const TYPE_DEPS_MASK: u64 = (1 << 56) - 1;

/// Struct containing the information passed from the vscode extension to the server
/// when the extension wants mmb debugger information. 
#[derive(Debug, Clone, Eq, PartialEq, Deserialize)]
pub(crate) struct MmbDebugParams {
    pub(crate) file_uri: String,
    pub(crate) decl_ident: String,
    pub(crate) elab_level: u8,
    pub(crate) bracket_level: u8,
    pub(crate) stepnum: Option<usize>,
    pub(crate) unify_req: bool,
    pub(crate) table: bool,
}

// Returns true if a value with type 'from' can be cast to a value of type 'to'.
// This requires that the sorts be the same, and additionally if 'to' is a
// name then so is 'from'.
pub(in crate::mmb::debugger) fn sorts_compatible(from: Type, to: Type) -> bool {
  let (from, to) = (from.into_inner(), to.into_inner());
  let diff = from ^ to;
  let c1 = || (diff & !TYPE_DEPS_MASK) == 0;
  let c2 = || (diff & !TYPE_BOUND_MASK & !TYPE_DEPS_MASK) == 0;
  let c3 = || ((from & TYPE_BOUND_MASK) != 0);
  c1() || (c2() && c3())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum MmbExpr<'b> {
    Var {
        idx: usize,
        ty: Type
    },
    App {
        term_num: TermId,
        args: &'b [&'b MmbItem<'b>],
        ty: Type,
    },
}

// Stack item
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum MmbItem<'b> {
    Expr(&'b MmbExpr<'b>),
    Proof(&'b MmbItem<'b>),
    Conv(&'b MmbItem<'b>, &'b MmbItem<'b>),
    CoConv(&'b MmbItem<'b>, &'b MmbItem<'b>)
}


pub(crate) fn verify1_extern(mmb: &MmbFile<'_>, mm1_env: &Environment, stmt: NumdStmtCmd, proof: ProofIter<'_>, params: MmbDebugParams) -> Result<Value, String> {
    verify1_extern_aux(mmb, mm1_env, stmt, proof, params).map_err(|e| format!("{:?}", e))
}
/// Returns Result<serde_json::Value, VerifErr>. The outer error here (the E in Result<A, E>) is only thrown
/// if there was something wrong with the actual request. If there was an error during the verification, that gets
/// returned in the Ok(json value) response so that the server can return both the verification error and the
/// states of the verifier leading up to the error.
pub(in crate::mmb::debugger) fn verify1_extern_aux(mmb: &MmbFile<'_>, mm1_env: &Environment, stmt: NumdStmtCmd, proof: ProofIter<'_>, params: MmbDebugParams) -> Res<Value> {
    let mut bump = Bump::new();
    match stmt {
        NumdStmtCmd::Sort {..} => { 
            if !proof.is_null() {
                return Err(VerifErr::Msg(format!("mmb sorts must have null proof iterators")));
            }
        },
        NumdStmtCmd::TermDef { term_id, .. } => {
            let term = none_err!(mmb.term(term_id))?;
            if !term.def() && !proof.is_null() {
                return Err(VerifErr::Msg(format!("mmb terms must have null proof iterators")))?;
            }
            let mut state = MmbState::new_from(stmt, mmb, mm1_env, &mut bump, params)?;
            let verif_result = state.verify_termdef(stmt, term, proof);
            let snapshots = state.mk_response(verif_result.err())?;
            return Ok(snapshots)
        }
        NumdStmtCmd::Axiom { thm_id, .. } | NumdStmtCmd::Thm { thm_id, ..  } => {
            let assert = none_err!(mmb.thm(thm_id))?;
            let mut state = MmbState::new_from(stmt, mmb, mm1_env, &mut bump, params)?;
            let verif_result = state.verify_assert(stmt, assert, proof);
            let snapshots = state.mk_response(verif_result.err())?;
            return Ok(snapshots)
        }            
    }
    Err(VerifErr::Msg(format!("unrecognized StmtCmd: {:?}", stmt)))
}



impl<'b, 'a : 'b> MmbItem<'b> {
    pub(in crate::mmb::debugger) fn get_ty(&self) -> Res<Type> {
        match self {
            | MmbItem::Expr(MmbExpr::Var { ty, .. })
            | MmbItem::Expr(MmbExpr::App { ty, ..}) => Ok(*ty),
            _ => Err(VerifErr::Msg(format!("Can't get type from a non-expr MmbItem")))
        }
    }

    pub(in crate::mmb::debugger) fn get_deps(&self) -> Res<Type> {
        self.get_ty()
        .and_then(|ty| none_err!(ty.deps()))
        .map(|deps| Type::from(deps))
    }

    pub(in crate::mmb::debugger) fn get_bound_digit(&self) -> Res<Type> {
        self.get_ty()
        .and_then(|ty| none_err!(ty.bound_digit()))
        .map(|bound_idx| Type::from(bound_idx))
    }

    pub(in crate::mmb::debugger) fn low_bits(&self) -> Type {
        self.get_deps().or(self.get_bound_digit()).unwrap()
    }    
}

// Write a table row to the string representing the aggregate
// of a table's rows. This is just extracted because it contains a 
// large/typo-prone format invocation.
fn write_row(
    table: &mut String,
    class: &str,
    n: usize,
    cmd: String,
    mode: Either<Mode, UMode>,
    data: &str
) {
    write!(
        table,
        "<tr class=\"{class}\" id=\"r{id}\"><td id=\"n{id}\">{step}</td><td>{cmd}</td><td>{mode}</td><td>{data}</td></tr>", 
        class=class,
        id = n, 
        step = n, 
        cmd = cmd,
        mode = mode,
        data = data
    ).unwrap();
}

struct Binder<'b> {
    name: Cow<'b, str>,
    ty: Option<Type>,
    dummy: bool
}

impl<'b> Binder<'b> {
    fn new(name: impl Into<Cow<'b, str>>, ty: Option<Type>, dummy: bool) -> Self {
        Binder {
            name: name.into(),
            ty,
            dummy
        }
    }
}

struct StmtBinders<'b> {
    pub(in crate::mmb::debugger) binder_names: VarListRef<'b>,
    pub(in crate::mmb::debugger) stmt_vars: Vec<(usize, Binder<'b>)>
}

// Dealing with the binders is a little bit tricky; the main issue is dummy variables,
// of which there are two kinds, named and unnamed. For the named ones, we know they exist,
// and we can get the name from `stmt_vars`, but we can't get their type directly since
// they're not in the `args` iter. For the unnamed ones, we won't even know they
// exist until we start running the proof/unify streams, but at that point we'll have 
// the type, and we can assign it a generic name like `v6` for a dummy variable that points
// to the 6th heap element.
impl<'b, 'a> StmtBinders<'b> {
    fn new(
        mmb_file: &'b MmbFile<'a>,
        stmt: NumdStmtCmd,
    ) -> Self {
        let binder_names = mmb_file.stmt_vars(stmt);
        let mut stmt_vars = Vec::new();
        let get_arg = |n: usize| match stmt {
            NumdStmtCmd::TermDef { term_id, .. } => mmb_file.term(term_id).unwrap().args().get(n).copied(),
            | NumdStmtCmd::Axiom { thm_id, .. } 
            | NumdStmtCmd::Thm { thm_id, .. } => mmb_file.thm(thm_id).unwrap().args().get(n).copied(),
            _ => unreachable!()
        };

        for i in 0.. {
            if let Some(var_name) = binder_names.get_opt(i) {
                let ty = get_arg(i);
                stmt_vars.push((i, Binder::new(var_name, ty, ty.is_none())));
            } else {
                break
            }
        }

        StmtBinders {
            binder_names,
            stmt_vars,
        }
    }

    fn get(&self, idx: usize) -> Option<&(usize, Binder<'b>)> {
        self.stmt_vars.iter().find(|(i, _)| *i == idx)
    }

    fn bound_vars(&self) -> impl Iterator<Item = &(usize, Binder<'b>)> {
        self.stmt_vars.iter().filter(|(_, bi)| bi.ty.map(|t| t.bound()) == Some(true))
    }

    // As we run through the proof streams (for thms) and the unify stream (for defs)
    // we'll eventually learn the assigned types of both named and unnamed dummy variables.
    // At that point we want to assign them the type recognized by the verifier.
    // In the case of a named dummy variable, we'll already have a slot with the 
    // name the user gave it in the mm1 file. For an unnamed dummy variable, it will
    // be completely new.
    fn set_dummy(&mut self, idx: usize, ty: Type) {
        match self.stmt_vars.iter_mut().find(|(i, ..)| *i == idx) {
            Some((_, b)) => {
                assert!(b.dummy);
                assert!(b.ty.is_none());
                b.ty = Some(ty);
            },
            None => {
                let name = self.binder_names.get(idx);
                self.stmt_vars.push((idx, Binder::new(name, Some(ty), true)))
            }
        }
    }
}

pub(in crate::mmb::debugger) struct MmbState<'b, 'a> {
    pub(in crate::mmb::debugger) stmt: NumdStmtCmd,
    pub(in crate::mmb::debugger) mm1_env: &'b Environment,
    pub(in crate::mmb::debugger) stmt_binders: StmtBinders<'b>,
    pub(in crate::mmb::debugger) mmb_file: &'b MmbFile<'a>,
    pub(in crate::mmb::debugger) bump: &'b Bump,
    pub(in crate::mmb::debugger) stack:  BumpVec<'b, &'b MmbItem<'b>>,
    pub(in crate::mmb::debugger) heap  : BumpVec<'b, &'b MmbItem<'b>>,
    pub(in crate::mmb::debugger) ustack: BumpVec<'b, &'b MmbItem<'b>>,
    pub(in crate::mmb::debugger) uheap : BumpVec<'b, &'b MmbItem<'b>>,
    pub(in crate::mmb::debugger) hstack: BumpVec<'b, &'b MmbItem<'b>>,     
    pub(in crate::mmb::debugger) next_bv: u64,
    pub(in crate::mmb::debugger) debug_params: MmbDebugParams,
    pub(in crate::mmb::debugger) cur_proof_step: usize,
    pub(in crate::mmb::debugger) cur_unify_step: usize,
    pub(in crate::mmb::debugger) cur_subunify_step: usize,
    pub(in crate::mmb::debugger) proof_step_checksum: Option<usize>,
    pub(in crate::mmb::debugger) unify_step_checksum: Option<usize>,
    // The states of the verifier as vectors of JSON values.
    pub(in crate::mmb::debugger) proof_states: Vec<Value>,
    pub(in crate::mmb::debugger) unify_states: Vec<Value>,
    // table rows
    pub(in crate::mmb::debugger) proof_rows: String,
    pub(in crate::mmb::debugger) unify_rows: String,
}

impl<'b, 'a> MmbState<'b, 'a> {
    pub(in crate::mmb::debugger) fn new_from(
        stmt: NumdStmtCmd,
        mmb_file: &'b MmbFile<'a>,
        mm1_env: &'b Environment,
        bump: &'b mut Bump,
        debug_params: MmbDebugParams
    ) -> Res<MmbState<'b, 'a>> {
        bump.reset();

        Ok(MmbState {
            stmt,
            mm1_env,
            stmt_binders: StmtBinders::new(mmb_file, stmt),
            mmb_file,
            bump: &*bump,
            stack : BumpVec::new_in(&*bump),
            heap  : BumpVec::new_in(&*bump),
            ustack: BumpVec::new_in(&*bump),
            uheap : BumpVec::new_in(&*bump),
            hstack: BumpVec::new_in(&*bump),
            next_bv: 1u64,
            cur_proof_step: 0usize,
            cur_unify_step: 0usize,
            cur_subunify_step: 0usize,
            proof_step_checksum: None,
            unify_step_checksum: None,
            debug_params,
            proof_states: Vec::new(),
            unify_states: Vec::new(),
            proof_rows: String::new(),
            unify_rows: String::new(),
        })
    }      

    pub(in crate::mmb::debugger) fn alloc<A>(&self, item: A) -> &'b A {
        &*self.bump.alloc(item)
    }

    pub(in crate::mmb::debugger) fn is_thm(&self) -> bool {
        matches!(self.stmt, NumdStmtCmd::Axiom {..} | NumdStmtCmd::Thm {..})
    }

    pub(in crate::mmb::debugger) fn targets_step(&self, n: usize) -> bool {
        if let Some(tgt) = self.debug_params.stepnum {
            (tgt.checked_sub(50).unwrap_or(0)..tgt.checked_add(50).unwrap_or(tgt)).contains(&n)
        } else {
            false
        }
    }

    pub(in crate::mmb::debugger) fn mk_response(&mut self, error: Option<VerifErr>) -> Res<Value> {
        let states = if self.debug_params.unify_req {
            std::mem::replace(&mut self.unify_states, Vec::new())
        } else {
            std::mem::replace(&mut self.proof_states, Vec::new())
        };

        if self.debug_params.table {
            let rows = if self.debug_params.unify_req {
                std::mem::replace(&mut self.unify_rows, String::new())
            } else {
                std::mem::replace(&mut self.proof_rows, String::new())
            };
            Ok(json!({
                "meta": self.get_metadata()?,
                "states": Value::Array(states),
                "table": Value::String(rows),
                "error": error.map(|e| format!("{:?}", e))
            }))
        } else {
            Ok(json!({
                "meta": self.get_metadata()?,
                "states": Value::Array(states),
                "table": Value::Null,
                "error": error.map(|e| format!("{:?}", e))
            }))
        }
    }

    pub(in crate::mmb::debugger) fn get_metadata(&mut self) -> Res<Value> {
        let decl_ident = self
            .mmb_file
            .stmt_index(self.stmt)
            .and_then(|nameref| nameref.value())
            .ok_or_else(|| err_msg!(format!("No statement found in index for cmd {:?}", self.stmt)))?;
        let (kind, declar_num, styled_decl_ident) = match self.stmt {
            mmb_parser::NumdStmtCmd::TermDef { term_id, .. } => {
                let is_def = self
                    .mmb_file
                    .term(term_id)
                    .ok_or_else(|| VerifErr::Msg(format!("mmb file had no data for term with id {:?}", term_id)))?
                    .def();
                if is_def {
                    ("def", term_id.into_inner(), format!("def <span class=\"def\">{}</span>", decl_ident))
                } else {
                    ("term", term_id.into_inner(), format!("term <span class=\"term\">{}</span>", decl_ident))
                }
            }
            mmb_parser::NumdStmtCmd::Axiom { thm_id, .. } => {
                ("axiom", thm_id.into_inner(), format!("axiom <span class=\"ax\">{}</span>", decl_ident))
            }
            mmb_parser::NumdStmtCmd::Thm { thm_id, .. } => {
                ("theorem", thm_id.into_inner(), format!("theorem <span class=\"thm\">{}</span>", decl_ident))
            }
            _ => unreachable!()
        };
        Ok(json!({
            "decl_kind": kind,
            "decl_num": declar_num,
            "decl_ident": decl_ident,
            "styled_decl_ident": styled_decl_ident,
            "total_proof_steps": self.cur_proof_step.checked_sub(1).unwrap_or(self.cur_proof_step),
            "total_unify_steps": self.cur_unify_step.checked_sub(1).unwrap_or(self.cur_unify_step),
            "vars": self.json_vars()?,
        }))
    }

    fn snap_proof(
        &mut self,
        mode: Mode,
        cmd: Option<ProofCmd>,
    ) -> Res<()> {
        
        if self.debug_params.table {
            let c = self.json_proof_cmd(cmd)?;
            write_row(&mut self.proof_rows, "proof_row", self.cur_proof_step, c, Either::L(mode), "--");
        }

        if self.targets_step(self.cur_proof_step) {
            let (stack, heap, ustack, uheap, hstack) = self.json_stores()?;
            let json = json!({
                "num": self.cur_proof_step,
                "mode": format!("{}", mode),
                "stack": stack,
                "heap": heap,
                "ustack": ustack,
                "uheap": uheap,
                "hstack": hstack,
                "cmd": self.json_proof_cmd(cmd)?,
            });
            self.proof_states.push(json);
        }
        self.cur_proof_step += 1;
        if cmd.is_none() {
            assert_eq!(self.proof_step_checksum.map(|x| x + 1), Some(self.cur_proof_step));
        }

        Ok(())
    }

    fn snap_unify(
        &mut self,
        uiter_len: usize,
        mode: UMode,
        cmd: Option<UnifyCmd>,
        tgt: &MmbItem<'_>,
        finish: Option<Finish<'_>>
    ) -> Res<()> {
        match finish {
            Some(finish) => self.snap_subunify(uiter_len, mode, cmd, tgt, finish),
            None => self.snap_proper_unify(mode, cmd, tgt),
        }
    }

    fn snap_subunify(
        &mut self,
        unify_len: usize,
        mode: UMode,
        cmd: Option<UnifyCmd>,
        tgt: &MmbItem<'_>,
        finish: Finish<'_>
    ) -> Res<()> {

        assert!(self.cur_subunify_step <= unify_len);

        if self.debug_params.table {
            let c = self.json_unify_cmd(cmd, true)?;
            write_row(&mut self.proof_rows, "subunify_row", self.cur_proof_step, c, Either::R(mode), "--");
        }

        if self.targets_step(self.cur_proof_step) && !self.debug_params.unify_req {
            let (stack, heap, ustack, uheap, hstack) = self.json_stores()?;
            let json = json!({
                "num": self.cur_proof_step,
                "mode": format!("{}", mode),
                "stack": stack,
                "heap": heap,
                "ustack": ustack,
                "uheap": uheap,
                "hstack": hstack,
                "cmd": self.json_unify_cmd(cmd, true)?,
                "subunify": json!({
                    "subnum": self.cur_subunify_step,
                    "subof": unify_len,
                    "tgt": self.as_sexpr(tgt)?,
                    "finish": self.json_finish(finish)?
                })
            });
            self.proof_states.push(json);
        }
        self.cur_proof_step += 1;
        if cmd.is_some() {
            self.cur_subunify_step += 1;
        } else {
            self.cur_subunify_step = 0;
        }
        Ok(())
    }

    fn snap_proper_unify(
        &mut self,
        mode: UMode,
        cmd: Option<UnifyCmd>,
        tgt: &MmbItem<'_>,
    ) -> Res<()> {

        if self.debug_params.table {
            let c = self.json_unify_cmd(cmd, false)?;
            write_row(&mut self.unify_rows, "unify_row", self.cur_unify_step, c, Either::R(mode), "--");
        }
        
        if self.targets_step(self.cur_unify_step) && self.debug_params.unify_req {
            let (stack, heap, ustack, uheap, hstack) = self.json_stores()?;
            let json = json!({
                "num": self.cur_unify_step,
                "mode": format!("{}", mode),
                "stack": stack,
                "heap": heap,
                "ustack": ustack,
                "uheap": uheap,
                "hstack": hstack,
                "cmd": self.json_unify_cmd(cmd, false)?,
                "tgt": self.as_sexpr(tgt)?,
            });
            self.unify_states.push(json);
        }
        self.cur_unify_step += 1;
        if cmd.is_none() {
            assert_eq!(self.unify_step_checksum.map(|x| x + 1), Some(self.cur_unify_step));
        }
        Ok(())
    }

    fn json_proof_cmd(&self, cmd: Option<ProofCmd>) -> Res<String> {
        let s = match cmd {
            Some(ProofCmd::Dummy(sort_id)) => {
                format!(
                    "ProofCmd::{:?} (<span class=\"sort\">{sort_name}</span>)", 
                    ProofCmd::Dummy(sort_id), 
                    sort_name = self.mmb_file.sort_name(sort_id)
                )
            }
            Some(ProofCmd::Term { tid, save }) => {
                format!(
                    "ProofCmd::{:?} (<span class=\"term\">{term_name}</span>, {num_args} args)", 
                    ProofCmd::Term { tid, save }, 
                    term_name = self.mmb_file.term_name(tid),
                    num_args = self.mmb_file.term(tid).map(|t| t.args().len())
                        .ok_or_else(|| err_msg!(format!("no term found for id: {:?}", tid)))?,
                        
                )
            }
            Some(ProofCmd::Thm { tid, save }) => {
                format!(
                    "ProofCmd::{:?} (<span class=\"thm\">{thm_name}</span>, {num_args} args)",
                    ProofCmd::Thm { tid, save },
                    thm_name = self.mmb_file.thm_name(tid),
                    num_args = self.mmb_file.thm(tid).map(|t| t.args().len())
                        .ok_or_else(|| err_msg!(format!("no thm found for id {:?}", tid)))?,
                )
            }
            Some(pcmd) => format!("ProofCmd::{:?}", pcmd),
            None => format!("Done!"),
        };
        Ok(s)
    }

    fn json_unify_cmd(&self, cmd: Option<UnifyCmd>, sub: bool) -> Res<String> {
        let s = match cmd {
            Some(UnifyCmd::Term {tid, save}) => {
                format!(
                    "UnifyCmd::{:?} (<span class=\"term\">{term_name}</span> {num_args} args)",
                    UnifyCmd::Term { tid, save }, 
                    term_name = self.mmb_file.term_name(tid),
                    num_args = self.mmb_file.term(tid).map(|t| t.args().len())
                        .ok_or_else(|| err_msg!(format!("no term found for id {:?}", tid)))?,
                )
            },
            Some(UnifyCmd::Dummy(sort_id)) => {
                format!("UnifyCmd::{:?} (<span class=\"sort\">{sort_name}</span>)", 
                    UnifyCmd::Dummy(sort_id), 
                    sort_name = self.mmb_file.sort_name(sort_id)
                )
            },
            Some(owise) => format!("UnifyCmd::{:?}", owise),
            None if sub => format!("Done (unify)!"),
            None => format!("Done!"),
        };
        Ok(s)
    }

    fn json_finish(&self, finish: Finish<'_>) -> Res<Value> {
        let s = match finish {
            Finish::Thm { proof_step, thm_id, save, proof } => {
                format!(
                    "{proof_step} {{ Thm <span class=\"thm\">{thm_name}</span>, save: {save} }} | {proof}",
                    proof_step = proof_step, 
                    thm_name = self.mmb_file.thm_name(thm_id),
                    save = save, 
                    proof = self.as_sexpr(proof)?
                )
            },
            Finish::Unfold { proof_step, e1, e2 } => {
                format!(
                    "{proof_step} Unfold | {e1_} == {e2_}", 
                    proof_step = proof_step, 
                    e1_ = self.as_sexpr(e1)?, 
                    e2_ = self.as_sexpr(e2)?
                )
            }
        };
        Ok(Value::String(s))
    }

    fn json_stack(&self, stack: &BumpVec<'b, &'b MmbItem<'b>>) -> Res<Value> {
        let mut s = String::new();
        for e in stack.iter() {
            write!(&mut s, "{open}{elem}{close}", open=STACK_OPEN, elem=self.as_sexpr(e)?, close=LI_CLOSE).unwrap();
        }
        Ok(Value::String(s))
    }

    fn json_heap(&self, heap: &BumpVec<'b, &'b MmbItem<'b>>) -> Res<Value> {
        let mut s = String::new();
        for (idx, e) in heap.iter().enumerate() {
            write!(&mut s, "{open}{idx} | {elem}{close}", open=HEAP_OPEN, idx=idx, elem=self.as_sexpr(e)?, close=LI_CLOSE).unwrap();
        }
        Ok(Value::String(s))
    }

    fn json_vars(&self) -> Res<Value> {
        let mut s = String::new();
        for (idx, binder) in self.stmt_binders.stmt_vars.iter() {
            let (open, close) = match binder.ty.map(|t| t.bound()) {
                None => panic!(),
                //Some(true) if binder.dummy => ("{<span class=\"dummy\">", "</span>}"),
                //Some(false) if binder.dummy => ("(<span class=\"dummy\">", "</span>)"),
                Some(true) => ("{<span class=\"bvar\">", "</span>}"),
                Some(false) => ("(<span class=\"var\">", "</span>)"),
            };
            let pfx = if binder.dummy { "." } else { "" };
            write!(
                &mut s,
                "{h_open}{n} | {open}{pfx}{name}: {ty}{close}{h_close}", 
                h_open = HEAP_OPEN,
                n = idx,
                open = open,
                pfx = pfx,
                name = binder.name,
                ty = self.print_type(binder.ty.unwrap())?,
                close = close,
                h_close = LI_CLOSE
            ).unwrap();
        }
        Ok(Value::String(s))
    }

    fn json_stores(&self) -> Res<(Value, Value, Value, Value, Value)> {
        Ok((
            self.json_stack(&self.stack)?,
            self.json_heap(&self.heap)?,
            self.json_stack(&self.ustack)?,
            self.json_heap(&self.uheap)?,
            self.json_stack(&self.hstack)?,
        ))
    }

    fn print_type(&self, ty: Type) -> Res<String> {
        let (sort_id, maybe_deps) = (ty.sort(), ty.deps());
        let mut acc = format!("<span class=\"sort\">{}</span>", self.mmb_file.sort_name(sort_id));
        if let Some(deps) = maybe_deps {
            for i in 0..56 {
                if (1 << i) & deps != 0 {
                    let depname = self
                        .stmt_binders
                        .bound_vars()
                        .nth(i)
                        .map(|(_, bi)| &bi.name)
                        .ok_or_else(|| err_msg!(format!("No bvar found for idx {}", i)))?;
                    acc.push_str(format!(" <span class=\"bvar\">{}</span>", depname).as_str());
                }
            }
        }
        Ok(acc)
    }

    fn print_bare(&self, term_num: TermId, args: &[&MmbItem<'b>]) -> Res<Paren> {
        let term_name = self.mmb_file.term_name(term_num);
        let mut acc = format!("<span class=\"term\">{}</span>", term_name);

        for arg in args.iter() {
            acc.push_str(format!(" {}", self.as_sexpr_level2(arg)?.paren_le(Val(10))).as_str());
        }
        Ok(Paren::always(acc))
    }

    fn print_app_aux(&self, args: &[&MmbItem<'b>], nota_info: &NotaInfo, acc: &mut String) -> Res<()> {
        for (idx, lit) in nota_info.lits.iter().enumerate() {
            match lit {
                Var(pos, _) => {
                    let this_var = args.get(*pos).ok_or_else(|| err_msg!(format!("no mmb item found in args @ {}", pos)))?;
                    let var_str = self.as_sexpr_level2(this_var)?.paren_le(Val(10));
                    acc.push_str(var_str.as_str());
                },
                Const(s) => { acc.push_str(s.as_str()); }
            }
            if !(idx + 1 == nota_info.lits.len()) {
                acc.push(' ');
            }
        }        
        Ok(())
    }

    fn bracket(&self, s: String, prio: u16) -> Paren {
        match self.debug_params.bracket_level {
            0 => Paren::never(s),
            _ => Paren::new(s, prio)
        }
    }

    fn print_app(&self, term_num: TermId, args: &[&MmbItem<'b>]) -> Res<Paren> {
        let pe = &self.mm1_env.pe;
        if let Some((_, v)) = pe.decl_nota.get(&term_num) {
            match v.as_slice() {
                // no declared notation
                [] => self.print_bare(term_num, args),
                // infix
                [(hd, true), ..] => {
                    let mut acc = String::new();
                    let infix = self.mm1_env.pe.infixes.get(hd).ok_or_else(|| err_msg!(format!("no infix found for {:?}", hd)))?;
                    self.print_app_aux(args, infix, &mut acc)?;
                    Ok(self.bracket(acc, 10))
                },
                // is prefix
                [(hd, false), ..] => {
                    let mut acc = format!("{}", hd);
                    let prefix = self.mm1_env.pe.prefixes.get(hd).ok_or_else(|| err_msg!(format!("no prefix found for {:?}", hd)))?;
                    match prefix.lits.len() {
                        // 0-ary prefix, like num lits `0`, `1`, etc.
                        0 => Ok(Paren::never(acc)),
                        // 1-ary prefix notation, like ~h
                        1 => {
                            self.print_app_aux(args, prefix, &mut acc)?;
                            Ok(self.bracket(acc, 20))
                        },
                        // 2+ary prefix/general notation.
                        _ => {
                            acc.push(' ');
                            self.print_app_aux(args, prefix, &mut acc)?;
                            Ok(self.bracket(acc, 15))
                        }
                    }
                }
            }
        } else {
            // didn't have any notation to begin with.
            self.print_bare(term_num, args)
        }
    }

    fn as_sexpr(&self, item: &MmbItem<'b>) -> Res<String> {
        match self.debug_params.elab_level {
            0 => self.as_sexpr_level0(item),
            1 => self.as_sexpr_level1(item),
            _ => self.as_sexpr_level2(item).map(|paren| paren.paren_le(Prio::Never)),
        }
    }


    // Elaborates variable names and notation.
    fn as_sexpr_level2(&self, item: &MmbItem<'b>) -> Res<Paren> {
        match item {
            MmbItem::Expr(MmbExpr::Var { idx, ty }) => {
                let (_, bi) = self
                .stmt_binders
                .get(*idx)
                .ok_or_else(|| err_msg!(format!("No binder found for index {}", idx)))?;
                let s = if ty.bound() {
                    //let open = if bi.dummy { DUMMY_OPEN } else { BVAR_OPEN };
                    format!("{open}{ident}{close}", open = BVAR_OPEN, ident = bi.name, close = SPAN_CLOSE)
                } else {
                    //let open = if bi.dummy { DUMMY_OPEN } else { VAR_OPEN };
                    format!("{open}{ident}{close}", open = VAR_OPEN, ident = bi.name, close = SPAN_CLOSE)
                };
                Ok(Paren::never(s))
            },
            MmbItem::Expr(MmbExpr::App { term_num, args, .. }) => self.print_app(*term_num, args),
            MmbItem::Proof(p) => {
                Ok(Paren::never(format!(
                    "{tstyle} {p_}",
                    tstyle = TSTYLE,
                    p_ = self.as_sexpr_level2(p)?.paren_le(Never)
                )))
            },
            MmbItem::Conv(c1, c2) => {
                Ok(Paren::never(format!(
                    "{c1_}{conv}{c2_}", 
                    c1_ = self.as_sexpr_level2(c1)?.paren_le(Never),
                    conv = CONV,
                    c2_ = self.as_sexpr_level2(c2)?.paren_le(Never)
                )))
            },
            MmbItem::CoConv(k1, k2) => {
                Ok(Paren::never(format!(
                    "{k1_}{coconv}{k2_}", 
                    k1_ = self.as_sexpr_level2(k1)?.paren_le(Never), 
                    coconv = COCONV,
                    k2_ = self.as_sexpr_level2(k2)?.paren_le(Never)
                )))
            },
        }
    }       

    // Elaborates the variable names, but doesn't do notation.
    fn as_sexpr_level1(&self, item: &MmbItem<'b>) -> Res<String> {
        match item {
            MmbItem::Expr(MmbExpr::Var { idx, ty }) => {
                let (_, bi) = self
                    .stmt_binders
                    .get(*idx)
                    .ok_or_else(|| err_msg!(format!("No binder found for index {}", idx)))?;
                if ty.bound() {
                    //let open = if bi.dummy { DUMMY_OPEN } else { BVAR_OPEN };
                    Ok(format!("{open}{ident}{close}", open = BVAR_OPEN, ident = bi.name, close = SPAN_CLOSE))
                } else {
                    //let open = if bi.dummy { DUMMY_OPEN } else { VAR_OPEN };
                    Ok(format!("{open}{ident}{close}", open = VAR_OPEN, ident = bi.name, close = SPAN_CLOSE))
                }
            },
            MmbItem::Expr(MmbExpr::App { term_num, args, .. }) => {
                let mut acc = format!("(<span class=\"term\">{}</span>", self.mmb_file.term_name(*term_num));

                for arg in args.iter() {
                    acc.push_str(format!(" {}", self.as_sexpr_level1(arg)?).as_str());
                }
                acc.push(')');
                Ok(acc)
            },
            MmbItem::Proof(p) => {
                Ok(format!("{tstyle} {p_}", tstyle=TSTYLE, p_ = self.as_sexpr_level1(p)?))
            },
            MmbItem::Conv(c1, c2) => {
                Ok(format!(
                    "{c1_}{open} = {close}{c2_}",
                    c1_ = self.as_sexpr_level1(c1)?, 
                    open = MMB_OPEN,
                    close = SPAN_CLOSE,
                    c2_ = self.as_sexpr_level1(c2)?
                ))
            },
            MmbItem::CoConv(k1, k2) => {
                Ok(format!(
                    "{k1_}{open} =?= {close}{k2_}",
                    k1_ = self.as_sexpr_level1(k1)?,
                    open = MMB_OPEN,
                    close = SPAN_CLOSE,
                    k2_ = self.as_sexpr_level1(k2)?
                ))
            },
        }
    }         

    // Elaborates neither variable names nor notation. Usable if you want a hard "sanity check"
    // that depends exclusively on the proof/unify streams themselves, and only the mmb parser.
    // IMO it's very rude to have a debugger gaslight users by giving them bad info, but by the time 
    // we get to elab level 1/2, we're depending on a lot more code, so for now this is the best
    // we can do.
    fn as_sexpr_level0(&self, item: &MmbItem<'b>) -> Res<String> {
        match item {
            MmbItem::Expr(MmbExpr::Var { idx, ty }) => {
                if ty.bound() {
                    Ok(format!("{open}{idx}{close}", open = BVAR_OPEN, idx = idx, close = SPAN_CLOSE))
                } else {
                    Ok(format!("{open}{idx}{close}", open = VAR_OPEN, idx = idx, close = SPAN_CLOSE))
                }
            },
            MmbItem::Expr(MmbExpr::App { term_num, args, .. }) => {
                let mut acc = format!("(<span class=\"term\">{}</span>", self.mmb_file.term_name(*term_num));

                for arg in args.iter() {
                    acc.push_str(format!(" {}", self.as_sexpr_level0(arg)?).as_str());
                }
                acc.push(')');
                Ok(acc)
            },
            MmbItem::Proof(p) => {
                Ok(format!("{tstyle} {p_}", tstyle=TSTYLE, p_ = self.as_sexpr_level0(p)?))
            },
            MmbItem::Conv(c1, c2) => {
                Ok(format!(
                    "{c1_}{open} = {close}{c2_}",
                    c1_ = self.as_sexpr_level0(c1)?, 
                    open = MMB_OPEN,
                    close = SPAN_CLOSE,
                    c2_ = self.as_sexpr_level0(c2)?
                ))
            },
            MmbItem::CoConv(k1, k2) => {
                Ok(format!(
                    "{k1_}{open} =?= {close}{k2_}",
                    k1_ = self.as_sexpr_level0(k1)?,
                    open = MMB_OPEN,
                    close = SPAN_CLOSE,
                    k2_ = self.as_sexpr_level0(k2)?
                ))
            },
        }
    }        
}

impl<'b, 'a> MmbState<'b, 'a> {
    fn take_next_bv(&mut self) -> u64 {
        let outgoing = self.next_bv;
        // Assert we're under the limit of 55 bound variables.
        assert!(outgoing >> 56 == 0);
        self.next_bv *= 2;
        outgoing
    }    

    fn load_args(&mut self, args: &[Arg], stmt: NumdStmtCmd) -> Res<()> {
        make_sure!(self.heap.len() == 0);
        make_sure!(self.next_bv == 1);

        for (idx, arg) in args.iter().enumerate() {
            if arg.bound() {
                // b/c we have a bound var, assert the arg's sort is not strict
                make_sure!(none_err!(self.mmb_file.sort(arg.sort()))?.0 & SORT_STRICT == 0);
                // increment the bv counter/checker
                let this_bv = self.take_next_bv();
                // assert that the mmb file has the right/sequential bv idx for this bound var
                make_sure!(none_err!(arg.bound_digit())? == this_bv);
            } else {
                // assert that this doesn't have any dependencies with a bit pos/idx greater
                // than the number of bvs that have been declared/seen.
                make_sure!(0 == (none_err!(arg.deps())? & !(self.next_bv - 1)));
            }

            self.heap.push(self.alloc(MmbItem::Expr(self.alloc(MmbExpr::Var { idx, ty: *arg }))));
        }
        // For termdefs, pop the last item (which is the return) off the stack.
        if let NumdStmtCmd::TermDef {..} = stmt {
            self.heap.pop();
        }
        Ok(())
    }       

    fn verify_termdef(
        &mut self, 
        stmt: NumdStmtCmd,
        term: TermRef<'a>,
        proof: ProofIter<'a>,
    ) -> Res<()> {
        self.load_args(term.args_and_ret(), stmt)?;
        if term.def() {
            self.run_proof(Mode::Def, proof)?;
            let final_val = none_err!(self.stack.pop())?;
            let ty = final_val.get_ty()?;
            make_sure!(self.stack.is_empty());
            make_sure!(sorts_compatible(ty, term.ret()));
            make_sure!(self.uheap.is_empty());
            for arg in self.heap.iter().take(term.args().len()) {
                self.uheap.push(*arg);
            }

            self.run_unify(UMode::UDef, term.unify(), final_val, None)?;
        }
        Ok(())
    }

    fn verify_assert(
        &mut self, 
        stmt: NumdStmtCmd,
        assert: ThmRef<'a>,
        proof: ProofIter<'a>,
    ) -> Res<()> {
        self.load_args(assert.args(), stmt)?;

        self.run_proof(Mode::Thm, proof)?;

        let final_val = match none_err!(self.stack.pop())? {
            MmbItem::Proof(p) if matches!(stmt, NumdStmtCmd::Thm {..}) => p,
            owise if matches!(stmt, NumdStmtCmd::Axiom {..}) => owise,
            owise => return Err(VerifErr::Msg(format!("Expected a proof; got {:?}", owise)))
        };

        make_sure!(self.stack.is_empty());
        make_sure!(self.uheap.is_empty());
        for arg in self.heap.iter().take(assert.args().len()) {
            self.uheap.push(*arg);
        }

        self.run_unify(UMode::UThmEnd, assert.unify(), final_val, None)
    }
}


// ad-hoc implementation of parens to prevent the need ot pull in a pretty-printer.
use Prio::*;

#[derive(Debug)]
enum Prio {
    Never,
    Val(u16),
    Always
}

struct Paren {
    s: String,
    prio: Prio
}

impl Paren {
    fn new(s: String, prio: u16) -> Self {
        Paren {
            s,
            prio: Prio::Val(prio)
        }
    }

    fn never(s: String) -> Self {
        Paren { s, prio: Never }
    }

    fn always(s: String) -> Self {
        Paren { s, prio: Always }
    }

    fn paren_le(self, upto: Prio) -> String {
        match (self.prio, upto) {
            | (Always, _) 
            | (_, Always) => format!("({})", self.s),
            | (Never, _) 
            | (_, Never) => self.s,
            | (Val(x), Val(y)) if x <= y => format!("({})", self.s),
            _ => self.s
        }
    }
}

