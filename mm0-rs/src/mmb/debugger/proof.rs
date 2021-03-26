use std::fmt;
use bumpalo::collections::Vec as BumpVec;

use crate::mmb::debugger::util::{ VerifErr, Res };
use crate::mmb::debugger::sorts_compatible;
use crate::mmb::debugger::unify::UMode;
use crate::mmb::debugger::{ MmbState, MmbItem, MmbExpr };
use mmb_parser::ty::Type;
use mmb_parser::ProofCmd;
use mmb_parser::parser::ProofIter;
use mm0_util::ids::{ SortId, TermId, ThmId };
use crate::none_err;
use crate::localize;
use crate::make_sure;

pub(in crate::mmb::debugger) const TYPE_BOUND_MASK: u64 = 1 << 63;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::mmb::debugger) enum Mode {
    Def,
    Thm,
}

impl fmt::Display for Mode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Mode::Def => write!(f, "Proof Def"),
            Mode::Thm => write!(f, "Proof Thm"),
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub(in crate::mmb::debugger) enum Finish<'b> {
    Thm {
        proof_step: usize,
        thm_id: ThmId,
        save: bool,
        proof: &'b MmbItem<'b>,
    },
    Unfold {
        proof_step: usize,
        e1: &'b MmbItem<'b>,
        e2: &'b MmbItem<'b>,
    }
    //Thm(usize, ProofCmd, &'b MmbItem<'b>, bool),
    //Unfold(usize, ProofCmd, &'b MmbItem<'b>, &'b MmbItem<'b>),
}

fn proof_len(p: &ProofIter<'_>) -> usize {
    let mut len = 0;
    let mut c = p.clone();
    while let Some(Ok(_)) = c.next() {
        len += 1;
    }
    len
}

impl<'b, 'a: 'b> MmbState<'b, 'a> {
    pub(in crate::mmb::debugger) fn run_proof(
        &mut self, 
        mode: Mode,
        proof: ProofIter<'_>
    ) -> Res<()> {    
        assert!(self.proof_step_checksum.replace(proof_len(&proof)).is_none());

        for maybe_cmd in proof {
            let cmd = maybe_cmd.map_err(|e| VerifErr::Msg(format!("{:?}", e)))?;
            self.snap_proof(mode, Some(cmd))?;
            match cmd {
                ProofCmd::Ref(i) => self.proof_ref(i)?,
                ProofCmd::Dummy(sort_num) => self.proof_dummy(sort_num)?,
                ProofCmd::Term { tid, save } => self.proof_term(mode, tid, save)?,
                ProofCmd::Thm { tid, save } => self.proof_thm(tid, save)?,
                ProofCmd::Hyp => self.proof_hyp(mode)?,
                ProofCmd::Conv => self.proof_conv()?,
                ProofCmd::Refl=> self.proof_refl()?,
                ProofCmd::Sym => self.proof_sym()?,
                ProofCmd::Cong => self.proof_cong()?,
                ProofCmd::Unfold => self.proof_unfold()?,
                ProofCmd::ConvCut => self.proof_conv_cut()?,
                ProofCmd::ConvRef(i) => self.proof_conv_ref(i)?,
                ProofCmd::ConvSave => self.proof_conv_save()?,
                ProofCmd::Save => self.proof_save()?,
                ProofCmd::Sorry => self.proof_sorry()?,
            }
        }
        // Make sure to capture the "Done" step
        self.snap_proof(mode, None)?;
        Ok(())
    }    

    fn proof_ref(&mut self, i: u32) -> Res<()> {
        let heap_elem = *&self.heap[i as usize];
        Ok(self.stack.push(heap_elem))
    }

    fn proof_dummy(&mut self, sort_num: SortId) -> Res<()> {
        make_sure!(sort_num.into_inner() < self.mmb_file.header.num_sorts);
        make_sure!(self.mmb_file.sort(sort_num).unwrap().0 & crate::mmb::debugger::SORT_STRICT == 0);
        // Owise too many bound variables.
        make_sure!(self.next_bv >> 56 == 0);

        let ty = Type::from(TYPE_BOUND_MASK | ((sort_num.into_inner() as u64) << 56) | self.take_next_bv());

        let idx = self.heap.len();
        let e = self.alloc(MmbItem::Expr(self.alloc(MmbExpr::Var { idx, ty })));
        if self.is_thm() {
            self.stmt_binders.set_dummy(idx, ty);
        }
        self.stack.push(e);
        Ok(self.heap.push(e))
    }        


    fn proof_term(
        &mut self, 
        mode: Mode,
        term_num: TermId,
        save: bool
    ) -> Res<()> {
        make_sure!(term_num.into_inner() < self.mmb_file.header.num_terms.get());
        let termref = none_err!(self.mmb_file.term(term_num))?;
        
        // remove ebar from the stack; either variables or applications.
        // We don't actually drain the elements from the stack until the end
        // in order to avoid an allocation.
        let drain_from = self.stack.len() - (termref.args().len());
        let stack_args = &self.stack[drain_from..];

        // (sig_args, stack_args)
        let all_args = || { termref.args().iter().zip(stack_args.iter()) };

        // Arguments from the stack (and their positions, starting from 1) that the stack demands be bound.
        let stack_bound_by_sig = all_args().filter_map(|(sig, stack)| {
            if sig.bound() {
                Some(stack.get_bound_digit())
            } else {
                None
            }
        });


        // For all of the args, make sure the stack and sig items have compatible sorts.
        for (sig_arg, stack_arg) in all_args() {
            make_sure!(sorts_compatible(stack_arg.get_ty()?, *sig_arg)) 
        }

        // Start building the new return type now that we know we have the right sort.
        let mut new_type_accum = Type::new_of_sort(termref.sort().into_inner());

        // For the args not bound by the signature...
        for (sig_unbound, stack_arg) in all_args().filter(|(sig, _)| !sig.bound()) {
            let mut stack_lowbits = stack_arg.get_deps().or(stack_arg.get_bound_digit())?;
            if mode == Mode::Def {
                for (idx, dep) in stack_bound_by_sig.clone().enumerate() {
                    if sig_unbound.depends_on((idx) as u64) {
                        stack_lowbits &= !(dep?);
                    }
                }
            }
            new_type_accum |= stack_lowbits
        }

        // For definitions with dependent return types, add the appropriate dependencies
        // to the type accumulator.
        if mode == Mode::Def && termref.ret().has_deps() {
            for (idx, bvar) in stack_bound_by_sig.enumerate() {
                if termref.ret().depends_on((idx) as u64) {
                    new_type_accum |= bvar?;
                }
            }
        }        

        // I think this will get around it.
        let drain = self.stack.drain((self.stack.len() - (termref.args().len()))..);
        let mut stack_args_out = BumpVec::new_in(self.bump);
        for elem in drain {
            stack_args_out.push(elem);
        }

        let t = self.alloc(MmbItem::Expr(self.alloc(MmbExpr::App {
            term_num,
            ty: new_type_accum,
            args: self.alloc(stack_args_out),
        })));

        if save {
            self.heap.push(t);
        }        
        Ok(self.stack.push(t))
    }       

    fn proof_thm(
        &mut self, 
        thm_num: ThmId,
        save: bool,
    ) -> Res<()> {
        make_sure!(thm_num.into_inner() < self.mmb_file.header.num_thms.get());
        let thmref = none_err!(self.mmb_file.thm(thm_num))?;
        let sig_args = thmref.args();

        let a = none_err!(self.stack.pop())?;

        // Wait to remove these in order to save an allocation.
        let drain_from = self.stack.len() - sig_args.len();
        let stack_args = &self.stack[drain_from..];

        let bound_by_sig = sig_args.iter().zip(stack_args).enumerate().filter(|(_, (sig, _))| sig.bound());

        self.uheap.extend(stack_args.into_iter());

        let mut bound_len = 0usize;
        // For each variable bound by the signature...
        for (idx, (_, stack_a)) in bound_by_sig.clone() {
            bound_len += 1;
            for j in 0..idx {
                make_sure!(*&self.uheap[j].get_ty().unwrap().disjoint(stack_a.low_bits()))
            }
        }

        // For the args not bound in the signature
        for (sig_a, stack_a) in  sig_args.iter().zip(stack_args).filter(|(sig, _)| !sig.bound()) {
            for j in 0..bound_len {
                make_sure!(
                    !(sig_a.disjoint(Type::from(1 << j)))
                    || bound_by_sig.clone().nth(j).unwrap().1.1.clone().low_bits().disjoint(stack_a.low_bits())
                )
            }
        }

        // Now we actually remove the stack_args from the stack
        self.stack.truncate(drain_from);
        self.run_unify(
            UMode::UThm, 
            thmref.unify(), 
            a, 
            Some(Finish::Thm { proof_step: self.cur_proof_step, thm_id: thm_num, save, proof: a })
            //Some(Finish::Thm(self.cur_proof_step, cmd, a, save))
        )?;

        let proof = self.alloc(MmbItem::Proof(a));
        if save {
            self.heap.push(proof);
        }
        Ok(self.stack.push(proof))
    }          
    

    fn proof_hyp(
        &mut self, 
        mode: Mode,
    ) -> Res<()> {
        make_sure!(mode != Mode::Def);
        let e = none_err!(self.stack.pop())?;
        //assert that e is in a provable sort since it's a hyp
        let e_sort_numx = e.get_ty()?.sort();
        let e_sort_mods = self.mmb_file.sort(e_sort_numx).unwrap().0;
        make_sure!(e_sort_mods & crate::mmb::debugger::SORT_PROVABLE != 0);
        self.hstack.push(e);
        let proof = self.alloc(MmbItem::Proof(e));
        Ok(self.heap.push(proof))
    }      


    fn proof_conv(&mut self) -> Res<()> {
        let e2proof = none_err!(self.stack.pop())?;
        let e1 = none_err!(self.stack.pop())?;
        match e2proof {
            MmbItem::Proof(conc) => {
                let e1proof = self.alloc(MmbItem::Proof(e1));
                self.stack.push(e1proof);
                let coconv_e1_e2 = self.alloc(MmbItem::CoConv(e1, conc));
                Ok(self.stack.push(coconv_e1_e2))
            },
            _ => return Err(VerifErr::Unreachable(file!(), line!()))
        }        
    }      

    fn proof_refl(&mut self) -> Res<()> {
        let e = none_err!(self.stack.pop())?;
        if let MmbItem::CoConv(cc1, cc2) = e {
            Ok(make_sure!((*cc1) as *const _ == (*cc2) as *const _))
        } else {
            return Err(VerifErr::Unreachable(file!(), line!()));
        }
    }      

    fn proof_sym(&mut self) -> Res<()> {
        let e = none_err!(self.stack.pop())?;
        if let MmbItem::CoConv(cc1, cc2) = e {
            let swapped = self.alloc(MmbItem::CoConv(cc2, cc1));
            Ok(self.stack.push(swapped))
        } else {
            return Err(VerifErr::Unreachable(file!(), line!()));
        }
    }      

    fn proof_cong(&mut self) -> Res<()> {
        let e = none_err!(self.stack.pop())?;
        if let MmbItem::CoConv(cc1, cc2) = e  {
            match (cc1, cc2) {
                (MmbItem::Expr(MmbExpr::App { term_num: n1, args: as1, .. }), MmbItem::Expr(MmbExpr::App { term_num: n2, args: as2, .. })) => {
                    make_sure!(n1 == n2);
                    make_sure!(as1.len() == as2.len());
                    for (lhs, rhs) in as1.iter().zip(as2.iter()).rev() {
                        let cc = self.alloc(MmbItem::CoConv(lhs, rhs));
                        self.stack.push(cc);
                    }
                    Ok(())
                },
                _ => return Err(VerifErr::Unreachable(file!(), line!()))
            }
        } else {
            return Err(VerifErr::Unreachable(file!(), line!()));
        }
    }      

    fn proof_unfold(&mut self) -> Res<()> {
        let e_prime = none_err!(self.stack.pop())?;
        let f_ebar = none_err!(self.stack.pop())?;
        let (term_num, ebar) = match f_ebar {
            MmbItem::Expr(MmbExpr::App{ term_num, args, .. }) => (term_num, args.clone()),
            _ => return Err(VerifErr::Unreachable(file!(), line!()))
        };

        make_sure!(self.uheap.is_empty());
        self.uheap.extend(ebar);

        self.run_unify(
            crate::mmb::debugger::unify::UMode::UDef,
            none_err!(self.mmb_file.term(*term_num))?.unify(),
            e_prime,
            Some(Finish::Unfold { proof_step: self.cur_proof_step, e1: f_ebar, e2: e_prime })
        )?;

        let cc = none_err!(self.stack.pop())?;
        if let MmbItem::CoConv(f_ebar2, e_doubleprime) = cc {
                make_sure!(f_ebar == *f_ebar2);
                let coconv = self.alloc(MmbItem::CoConv(e_prime, e_doubleprime));
                Ok(self.stack.push(coconv))
        } else {
            return Err(VerifErr::Unreachable(file!(), line!()));
        }
    }      

    fn proof_conv_cut(&mut self) -> Res<()> {
        let p = none_err!(self.stack.pop())?;
        if let MmbItem::CoConv(cc1, cc2) = p {
            let p1 = self.alloc(MmbItem::Conv(cc1, cc2));
            self.stack.push(p1);
            Ok(self.stack.push(p))
        } else {
            return Err(VerifErr::Unreachable(file!(), line!()));
        }
    }      

    fn proof_conv_ref(&mut self, i: u32) -> Res<()> {
        let heap_conv = none_err!(self.heap.get(i as usize).copied())?;
        let stack_coconv = none_err!(self.stack.pop())?;
        if let (MmbItem::Conv(c1, c2), MmbItem::CoConv(cc1, cc2)) = (heap_conv, stack_coconv) {
            make_sure!(c1 == cc1);
            Ok(make_sure!(c2 == cc2))
        } else {
            return Err(VerifErr::Unreachable(file!(), line!()));
        }
    }    

    fn proof_conv_save(&mut self) -> Res<()> {
        let p = localize!(none_err!(self.stack.pop()))?;
        make_sure!(matches!(p, MmbItem::Conv {..}));
        Ok(self.heap.push(p))
    }    

    fn proof_save(&mut self) -> Res<()> {
        let last = none_err!(self.stack.last().copied())?;
        match last {
            MmbItem::CoConv {..} => Err(VerifErr::Msg(format!("Can't save co-conv"))),
            _ => Ok(self.heap.push(last))
        }        
    }    

    fn proof_sorry(&mut self) -> Res<()> {
        match none_err!(self.stack.pop())? {
            e @ MmbItem::Expr(_) => {
                let proof = self.alloc(MmbItem::Proof(e));
                Ok(self.stack.push(proof))
            },
            MmbItem::Conv(..) => Ok(()),
            owise => Err(VerifErr::Msg(format!("ProofCmd::Sorry is only valid when the stack has an Expr or Conv on top. Top element was {:?}", owise)))
        }
    }    
}


