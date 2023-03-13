pub mod app;
pub use app::{Command as AppCommand, NetFn as AppNetFn};

pub mod connection;
use connection::{LogicalUnit, NetFn, NetFns, ParsedResponse, Request, Response};

pub mod storage;
pub use storage::{Command as StorageCommand, NetFn as StorageNetFn};
use storage::{SelAllocInfo, SelInfo};

#[macro_use]
mod fmt;
pub use fmt::{LogOutput, Loggable};

pub struct Ipmi<T> {
    inner: T,
    counter: i64,
}

impl<T> Ipmi<T> {
    pub fn new(inner: T) -> Self {
        Self { inner, counter: 0 }
    }
}

impl<T> From<T> for Ipmi<T> {
    fn from(value: T) -> Self {
        Self::new(value)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum IpmiError<T> {
    NetFnIsResponse(NetFn),
    IncorrectResponseSeq(i64, i64),
    UnexpectedResponse(NetFn, NetFn),
    ResponseParseFailed(<ParsedResponse as TryFrom<Response>>::Error),
    Connection(T),
}

impl<T> From<T> for IpmiError<T> {
    fn from(value: T) -> Self {
        Self::Connection(value)
    }
}

impl<T> Ipmi<T>
where
    T: connection::IpmiConnection,
{
    pub fn send_recv(
        &mut self,
        netfn: NetFn,
        data: &[u8],
    ) -> Result<ParsedResponse, IpmiError<T::Error>> {
        if netfn.is_response() {
            return Err(IpmiError::NetFnIsResponse(netfn));
        }

        let seq = self.counter;
        self.counter += 1;

        let request = Request::new(
            netfn,
            LogicalUnit::ONE,
            seq,
            data.iter().map(Clone::clone).collect(),
        );

        let response = self.inner.send_recv(&request)?;

        if response.seq() != seq {
            return Err(IpmiError::IncorrectResponseSeq(seq, response.seq()));
        }

        if !response.netfn().is_response_for(&netfn) {
            return Err(IpmiError::UnexpectedResponse(
                netfn.clone(),
                response.netfn().clone(),
            ));
        }

        response
            .try_into()
            .map_err(|e| IpmiError::ResponseParseFailed(e))
    }
}

macro_rules! get_parsed {
    ($($name:ident => $command:expr => $out:ty => $out_variant:ident),*) => {
        impl<T: connection::IpmiConnection> Ipmi<T> {
            $(
                pub fn $name(&mut self) -> Result<$out, IpmiError<T::Error>> {
                    let response = self.send_recv($command.into(), &[])?;

                    match response {
                        ParsedResponse::$out_variant(value) => Ok(value),
                        _ => unreachable!(),
                    }
                }
            )*
        }
    };
}

get_parsed!(
    get_sel_info => StorageNetFn::request(StorageCommand::GetSelInfo) => SelInfo => SelInfo,
    get_sel_alloc_info => StorageNetFn::request(StorageCommand::GetSelAllocInfo) => SelAllocInfo => SelAllocInfo
);
