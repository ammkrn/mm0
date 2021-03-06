
use std::ops::Range;
use serde_json::{ json, Value };
use bumpalo::Bump;
use bumpalo::collections::Vec as BumpVec;
use mm0_util::ids::TermId;
use mmb_parser::ty::{ Arg, Type };
use mm1_parser::ast::Binder;
use mmb_parser::{ MmbFile, NumdStmtCmd, ProofCmd, UnifyCmd };
use mmb_parser::parser::{ ProofIter, TermRef, ThmRef };

use crate::util::{ Res, VerifErr };
use crate::util::Either;
use crate::proof::{ Mode, Finish };
use crate::unify::UMode;

pub mod proof;
pub mod unify;
pub mod util;

// Each sort has one byte associated to it, which
// contains flags for the sort modifiers.
// The high four bits are unused.
pub const SORT_PURE     : u8 = 1;
pub const SORT_STRICT   : u8 = 2;
pub const SORT_PROVABLE : u8 = 4;
pub const SORT_FREE     : u8 = 8;

/// bound mask: 10000000_00000000_00000000_00000000_00000000_00000000_00000000_00000000
pub const TYPE_BOUND_MASK: u64 = 1 << 63;


/// deps mask: 00000000_11111111_11111111_11111111_11111111_11111111_11111111_11111111
pub const TYPE_DEPS_MASK: u64 = (1 << 56) - 1;


// Returns true if a value with type 'from' can be cast to a value of type 'to'.
// This requires that the sorts be the same, and additionally if 'to' is a
// name then so is 'from'.
pub fn sorts_compatible(from: Type, to: Type) -> bool {
  let (from, to) = (from.into_inner(), to.into_inner());
  let diff = from ^ to;
  let c1 = || (diff & !TYPE_DEPS_MASK) == 0;
  let c2 = || (diff & !TYPE_BOUND_MASK & !TYPE_DEPS_MASK) == 0;
  let c3 = || ((from & TYPE_BOUND_MASK) != 0);
  c1() || (c2() && c3())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MmbExpr<'b> {
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
pub enum MmbItem<'b> {
    Expr(&'b MmbExpr<'b>),
    Proof(&'b MmbItem<'b>),
    Conv(&'b MmbItem<'b>, &'b MmbItem<'b>),
    CoConv(&'b MmbItem<'b>, &'b MmbItem<'b>)
}


/// Returns Result<serde_json::Value, VerifErr>. The outer error here (the E in Result<A, E>) is only thrown
/// if there was something wrong with the actual request. If there was an error during the verification, that gets
/// returned in the Ok(json value) response so that the server can return both the verification error and the
/// states of the verifier leading up to the error.
pub fn verify1_extern(mmb: &MmbFile<'_>, stmt: NumdStmtCmd, proof: ProofIter<'_>, target_step: Option<usize>) -> Res<Value> {
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
            let mut state = MmbState::new_from(stmt, mmb, &mut bump, target_step);
            let verif_result = state.verify_termdef(stmt, term, proof);
            let snapshots = state.mk_response(verif_result.err());
            return Ok(snapshots)
        }
        NumdStmtCmd::Axiom { thm_id, .. } | NumdStmtCmd::Thm { thm_id, ..  } => {
            let assert = none_err!(mmb.thm(thm_id))?;
            let mut state = MmbState::new_from(stmt, mmb, &mut bump, target_step);
            let verif_result = state.verify_assert(stmt, assert, proof);
            let snapshots = state.mk_response(verif_result.err());
            return Ok(snapshots)
        }            
    }
    Err(VerifErr::Msg(format!("unrecognized StmtCmd: {:?}", stmt)))
}



impl<'b, 'a : 'b> MmbItem<'b> {
    pub fn get_ty(&self) -> Res<Type> {
        match self {
            | MmbItem::Expr(MmbExpr::Var { ty, .. })
            | MmbItem::Expr(MmbExpr::App { ty, ..}) => Ok(*ty),
            _ => Err(VerifErr::Msg(format!("Can't get type from a non-expr MmbItem")))
        }
    }

    pub fn get_deps(&self) -> Res<Type> {
        self.get_ty()
        .and_then(|ty| none_err!(ty.deps()))
        .map(|deps| Type::from(deps))
    }

    pub fn get_bound_digit(&self) -> Res<Type> {
        self.get_ty()
        .and_then(|ty| none_err!(ty.bound_digit()))
        .map(|bound_idx| Type::from(bound_idx))
    }

    pub fn low_bits(&self) -> Type {
        self.get_deps().or(self.get_bound_digit()).unwrap()
    }    
}



//#[derive(Debug)]
pub struct MmbState<'b, 'a: 'b> {
    pub stmt: NumdStmtCmd,
    pub mmz_vars: Vec<(&'a str, Binder)>,
    pub mmb_file: &'b MmbFile<'a>,
    pub bump: &'b Bump,
    pub stack:  BumpVec<'b, &'b MmbItem<'b>>,
    pub heap  : BumpVec<'b, &'b MmbItem<'b>>,
    pub ustack: BumpVec<'b, &'b MmbItem<'b>>,
    pub uheap : BumpVec<'b, &'b MmbItem<'b>>,
    pub hstack: BumpVec<'b, &'b MmbItem<'b>>,     
    pub next_bv: u64,
    pub cur_step: usize,
    pub desired_steps: Option<Range<usize>>,
    pub json: Vec<Value>,
}

impl<'b, 'a: 'b> MmbState<'b, 'a> {
    pub fn alloc<A>(&self, item: A) -> &'b A {
        &*self.bump.alloc(item)
    }

    pub fn mk_response(&mut self, error: Option<VerifErr>) -> Value {

        // add a final step to show the end condition with no steps remaining.
        if (self.desired_steps.as_ref().map(|range| range.contains(&self.cur_step)).unwrap_or(false)) {
            let snap = self.json_snapshot(None);
            self.json.push(snap);
        }
        self.cur_step += 1;

        let v = std::mem::replace(&mut self.json, Vec::new());
        json!({
            "metadata": self.get_metadata(),
            "states": Value::Array(v),
            "error": error.map(|e| format!("{:?}", e))
        })
    }

    pub fn get_metadata(&self) -> Value {
        let decl_ident = self.mmb_file.index.as_ref().unwrap().stmt(self.stmt).unwrap().value().unwrap();
        let (kind, declar_num, styled_decl_ident) = match self.stmt {
            mmb_parser::NumdStmtCmd::TermDef { term_id, .. } => {
                let is_def = self.mmb_file.term(term_id).unwrap().def();
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

        json!({
            "decl_kind": kind,
            "decl_num": declar_num,
            "decl_ident": decl_ident,
            "styled_decl_ident": styled_decl_ident,
            "num_steps_offset": self.cur_step.checked_sub(1).unwrap_or(self.cur_step),
        })
    }

    pub fn try_snapshot(&mut self, cmd: Either<ProofCmd, (UnifyCmd, &MmbItem<'_>, Option<Finish<'_>>)>) {
        if (self.desired_steps.as_ref().map(|range| range.contains(&self.cur_step)).unwrap_or(false)) {
            let snap = self.json_snapshot(Some(cmd));
            self.json.push(snap);
        }
        self.cur_step += 1;
    }

    pub fn new_from(stmt: NumdStmtCmd, mmb_file: &'b MmbFile<'a>, bump: &'b mut Bump, target_step: Option<usize>) -> MmbState<'b, 'a> {
        bump.reset();
        MmbState {
            stmt,
            mmz_vars: Vec::new(),
            mmb_file,
            bump: &*bump,
            stack : BumpVec::new_in(&*bump),
            heap  : BumpVec::new_in(&*bump),
            ustack: BumpVec::new_in(&*bump),
            uheap : BumpVec::new_in(&*bump),
            hstack: BumpVec::new_in(&*bump),
            next_bv: 1u64,
            cur_step: 0usize,
            desired_steps: target_step.map(|t| (t.checked_sub(50).unwrap_or(0))..(t.checked_add(50).unwrap_or(t))),
            json: Vec::new(),
        }
    }    

    pub fn json_snapshot(&mut self, cmd: Option<Either<ProofCmd, (UnifyCmd, &MmbItem<'_>, Option<Finish<'_>>)>>) -> Value {
        let (cmd_string, tgt, finish) = match cmd {
            Some(Either::L(ProofCmd::Dummy(sort_id))) => {
                let sort_name = self.mmb_file.index.as_ref().unwrap().sort(sort_id).unwrap().value().unwrap();
                (format!("ProofCmd::{:?} (<span class=\"sort\">{}</span>)", ProofCmd::Dummy(sort_id), sort_name), String::new(), String::new())
            }
            Some(Either::L(ProofCmd::Term { tid, save })) => {
                let term_name = self.mmb_file.index.as_ref().unwrap().term(tid).unwrap().value().unwrap();
                let num_args = self.mmb_file.term(tid).unwrap().args().len();
                (format!("ProofCmd::{:?} (<span class=\"term\">{}</span> (num args: {}))", ProofCmd::Term { tid, save }, term_name, num_args), String::new(), String::new())
            }
            Some(Either::L(ProofCmd::Thm { tid, save })) => {
                let thm_name = self.mmb_file.index.as_ref().unwrap().thm(tid).unwrap().value().unwrap();
                let num_args = self.mmb_file.thm(tid).unwrap().args().len();
                (format!("ProofCmd::{:?} (<span class=\"thm\">{}</span> (num args: {}))", ProofCmd::Thm { tid, save }, thm_name, num_args), String::new(), String::new())
            }
            Some(Either::L(pcmd)) => (format!("ProofCmd::{:?}", pcmd), String::new(), String::new()),
            Some(Either::R((ucmd, tgt, finish))) => {
                let cmd_string = match ucmd {
                    UnifyCmd::Term {tid, save} => {
                        let term_name = self.mmb_file.index.as_ref().unwrap().term(tid).unwrap().value().unwrap();
                        let num_args = self.mmb_file.term(tid).unwrap().args().len();
                        format!("UnifyCmd::{:?} (<span class=\"term\">{}</span> (num args: {}))", UnifyCmd::Term { tid, save }, term_name, num_args)
                    },
                    UnifyCmd::Dummy(sort_id) => {
                        let sort_name = self.mmb_file.index.as_ref().unwrap().sort(sort_id).unwrap().value().unwrap();
                        format!("UnifyCmd::{:?} (<span class=\"sort\">{}</span>)", UnifyCmd::Dummy(sort_id), sort_name)
                    },
                    owise => {
                        format!("UnifyCmd::{:?}", owise)
                    }
                };
                match (tgt, finish) {
                    (tgt, None) => (cmd_string, self.as_sexpr_level0(tgt), String::new()),
                    (tgt, Some(Finish::Thm(p, save))) => (cmd_string, self.as_sexpr_level0(tgt), format!("{}; {}", save, self.as_sexpr_level0(p))),
                    (tgt, Some(Finish::Unfold(e1, e2))) => (cmd_string, self.as_sexpr_level0(tgt), format!("{}; {}", self.as_sexpr_level0(e1), self.as_sexpr_level0(e2)))
                }
            },
            None => (format!("Done"), String::new(), String::new())
        };
        let stack = Value::Array(self.stack.iter().map(|x| Value::String(self.as_sexpr_level0(x))).collect::<Vec<Value>>());
        let heap = Value::Array(self.heap.iter().map(|x| Value::String(self.as_sexpr_level0(x))).collect::<Vec<Value>>());
        let ustack = Value::Array(self.ustack.iter().map(|x| Value::String(self.as_sexpr_level0(x))).collect::<Vec<Value>>());
        let uheap = Value::Array(self.uheap.iter().map(|x| Value::String(self.as_sexpr_level0(x))).collect::<Vec<Value>>());
        let hstack= Value::Array(self.hstack.iter().map(|x| Value::String(self.as_sexpr_level0(x))).collect::<Vec<Value>>());

        json!({
            "stepnum": self.cur_step,
            "stack": stack,
            "heap": heap,
            "ustack": ustack,
            "uheap": uheap,
            "hstack": hstack,
            "vars": self.mmz_vars.iter().map(|x| x.0).collect::<Vec<&str>>(),
            "cmd": cmd_string,
            "tgt": tgt,
            "finish": finish,
        })
    }

    pub fn as_sexpr_level0(&self, item: &MmbItem<'b>) -> String {
        match item {
            MmbItem::Expr(MmbExpr::Var { idx, ty }) => {
                if ty.bound() {
                    format!("<span class=\"bvar\">{}</span>", idx)
                } else {
                    format!("<span class=\"var\">{}</span>", idx)
                }
            },
            MmbItem::Expr(MmbExpr::App { term_num, args, .. }) => {
                let term_name = self.mmb_file.index.as_ref().unwrap().term(*term_num).unwrap().value().unwrap();
                let mut acc = format!("(<span class=\"term\">{}</span>", term_name);

                for arg in args.iter() {
                    acc.push_str(format!(" {}", self.as_sexpr_level0(arg)).as_str());
                }
                acc.push(')');
                acc
            },
            MmbItem::Proof(p) => {
                format!("(P {})", self.as_sexpr_level0(p))
            },
            MmbItem::Conv(c1, c2) => {
                format!("(C {} {})", self.as_sexpr_level0(c1), self.as_sexpr_level0(c2))
            },
            MmbItem::CoConv(k1, k2) => {
                format!("(K {} {})", self.as_sexpr_level0(k1), self.as_sexpr_level0(k2))
            },
        }
    }        
}

impl<'b, 'a: 'b> MmbState<'b, 'a> {
    pub fn take_next_bv(&mut self) -> u64 {
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
                make_sure!(self.mmb_file.sort(arg.sort()).unwrap().0 & SORT_STRICT == 0);
                // increment the bv counter/checker
                let this_bv = self.take_next_bv();
                // assert that the mmb file has the right/sequential bv idx for this bound var
                make_sure!(none_err!(arg.bound_digit())? == this_bv);
            } else {
                // assert that this doesn't have any dependencies with a bit pos/idx greater
                // than the number of bvs that have been declared/seen.
                make_sure!(0 == (arg.deps().unwrap() & !(self.next_bv - 1)));
            }

            self.heap.push(self.alloc(MmbItem::Expr(self.alloc(MmbExpr::Var { idx, ty: *arg }))));
        }
        // For termdefs, pop the last item (which is the return) off the stack.
        if let NumdStmtCmd::TermDef {..} = stmt {
            self.heap.pop();
        }
        Ok(())
    }       

    pub fn verify_termdef(
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

    pub fn verify_assert(
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

