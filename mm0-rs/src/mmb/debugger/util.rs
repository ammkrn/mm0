use std::fmt;


#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(in crate::mmb::debugger) enum Either<A, B> {
    L(A),
    R(B),
}

impl<A: fmt::Display, B: fmt::Display> fmt::Display for Either<A, B> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Either::L(a) => write!(f, "{}", a),
            Either::R(b) => write!(f, "{}", b),
        }
    }
}

pub(in crate::mmb::debugger) type Res<A> = Result<A, VerifErr>;

//#[derive(Clone)]
pub(in crate::mmb::debugger) enum VerifErr {
    MakeSure(&'static str, u32),
    NoneErr(&'static str, u32),
    ConvErr(&'static str, u32),
    Msg(String),
    LocalMsg(&'static str, u32, String),
    // Crate a rough backtrace; use the `localize!` macro to make this.
    Local(&'static str, u32, Box<VerifErr>),
    Unreachable(&'static str, u32),
    IoErr(&'static str, u32, std::io::Error)
}



impl fmt::Debug for VerifErr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            VerifErr::Msg(s) => {
                let mut d = f.debug_struct("VerifErr::Msg");
                d.field("msg", &format_args!("{}", s));
                d.finish()
            },
            VerifErr::LocalMsg(file, line, s) => {
                let mut d = f.debug_struct("VerifErr::LocMsg");
                d.field("file", &file);
                d.field("line", &line);
                d.field("msg", &format_args!("{}", s));
                d.finish()
            },
            VerifErr::Local(fi, l, e) => {
                let mut d = f.debug_struct("VerifErr::Local");
                d.field("file", &fi);
                d.field("line", &l);
                d.field("msg", &e);
                d.finish()
            },
            VerifErr::MakeSure(fi, l) => {
                let mut d = f.debug_struct("VerifErr::MakeSure");
                d.field("file", &fi);
                d.field("line", &l);
                d.finish()
            },
            VerifErr::ConvErr(fi, l) => {
                let mut d = f.debug_struct("VerifErr::ConvErr");
                d.field("file", &fi);
                d.field("line", &l);
                d.finish()
            },
            VerifErr::Unreachable(fi, l) => {
                let mut d = f.debug_struct("VerifErr::Unreachable");
                d.field("file", &fi);
                d.field("line", &l);
                d.finish()
            },
            VerifErr::IoErr(fi, l, e) => {
                let mut d = f.debug_struct("VerifErr::IoErr");
                d.field("file", &fi);
                d.field("line", &l);
                d.field("err", &format_args!("{}", e));
                d.finish()
            },
            VerifErr::NoneErr(fi, l) => {
                let mut d = f.debug_struct("VerifErr::NoneErr");
                d.field("file", &fi);
                d.field("line", &l);
                d.finish()
            },
        }
    }
}
