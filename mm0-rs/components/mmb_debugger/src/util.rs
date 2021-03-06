use std::fmt::{ Debug, Formatter, Result as FmtResult };

/// 10000000_11111111_11111111_11111111_11111111_11111111_11111111_11111111
const TYPE_SORT_MASK: u64 = (1 << 63) | ((1 << 56) - 1);


#[macro_export]
macro_rules! localize {
    ( $e:expr ) => {
        $e.map_err(|e| VerifErr::Local(file!(), line!(), Box::new(e)))
    }
}

#[macro_export]
macro_rules! io_err {
    ( $e:expr ) => {
        $e.map_err(|e| VerifErr::IoErr(file!(), line!(), Box::new(e)))
    }
}

#[macro_export]
macro_rules! none_err {
    ( $e:expr ) => {
        $e.ok_or(VerifErr::NoneErr(file!(), line!()))
    }
}

#[macro_export]
macro_rules! conv_err {
    ( $e:expr ) => {
        $e.map_err(|_| VerifErr::ConvErr(file!(), line!()))
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

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Either<A, B> {
    L(A),
    R(B),
}

pub type Res<A> = Result<A, VerifErr>;

//#[derive(Clone)]
pub enum VerifErr {
    MakeSure(&'static str, u32),
    NoneErr(&'static str, u32),
    ConvErr(&'static str, u32),
    Msg(String),
    // Crate a rough backtrace; use the `localize!` macro to make this.
    Local(&'static str, u32, Box<VerifErr>),
    Unreachable(&'static str, u32),
    IoErr(&'static str, u32, std::io::Error)
}

impl Debug for VerifErr {
    fn fmt(&self, f: &mut Formatter) -> FmtResult {
        match self {
            VerifErr::Msg(s) => {
                let mut d = f.debug_struct("VerifErr::Msg");
                d.field("Msg", &format_args!("{}", s));
                d.finish()
            },
            VerifErr::Local(fi, l, e) => {
                let mut d = f.debug_struct("VerifErr::Local");
                d.field("file", &fi);
                d.field("line", &l);
                d.field("Msg", &e);
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
