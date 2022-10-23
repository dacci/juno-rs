use super::*;
use bytes::{Buf, BufMut, Bytes, BytesMut};
use std::io::BufRead;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

#[derive(Debug, Clone, PartialEq, Eq)]
enum Request {
    Connect(SocketAddr, String),
    Bind(SocketAddr, String),
}

impl Request {
    fn from_buf<B: Buf>(buf: &mut B) -> Result<Self> {
        if buf.remaining() < 8 {
            return Err(Error::NeedMoreData);
        }

        let code = match buf.get_u8() {
            code @ (1 | 2) => code,
            code => return Err(Error::Protocol(format!("illegal request `{code}`"))),
        };

        let port = buf.get_u16();
        let ip = buf.get_u32();
        let user = Self::read_string(buf)?;

        let addr = if (1..0x100).contains(&ip) {
            SocketAddr::raw(Self::read_string(buf)?, port)
        } else {
            SocketAddr::v4(ip, port)
        };

        match code {
            1 => Ok(Request::Connect(addr, user)),
            2 => Ok(Request::Bind(addr, user)),
            _ => unreachable!(),
        }
    }

    fn read_string<B: Buf>(buf: &mut B) -> Result<String> {
        let mut vec = vec![];
        let len = buf.reader().read_until(0, &mut vec)?;
        if vec[len - 1] != 0 {
            return Err(Error::NeedMoreData);
        }

        vec.pop();
        String::from_utf8(vec).map_err(|e| Error::Protocol(e.to_string()))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Response {
    Granted,
    Rejected,
}

impl From<Response> for Bytes {
    fn from(res: Response) -> Self {
        let mut buf = BytesMut::with_capacity(8);

        buf.put_u8(0);

        match res {
            Response::Granted => buf.put_u8(90),
            Response::Rejected => buf.put_u8(91),
        }

        buf.put_u16(0);
        buf.put_u32(0);

        buf.freeze()
    }
}

pub async fn handle_request(mut client: TcpStream) -> Result<()> {
    let request = read_request(&mut client).await?;

    let (server, response) = match request {
        Request::Connect(addr, _) => {
            let res = match addr {
                SocketAddr::V4(addr) => TcpStream::connect(addr).await,
                SocketAddr::Raw(domain, port) => TcpStream::connect((domain, port)).await,
                _ => unreachable!(),
            };

            if let Ok(server) = res {
                (Some(server), Response::Granted)
            } else {
                (None, Response::Rejected)
            }
        }
        Request::Bind(_, _) => (None, Response::Rejected),
    };

    let mut buf: Bytes = response.into();
    client.write_all_buf(&mut buf).await?;

    if let Some(mut server) = server {
        io::copy_bidirectional(&mut client, &mut server).await?;
    }

    Ok(())
}

async fn read_request(stream: &mut TcpStream) -> Result<Request> {
    let mut buf = BytesMut::with_capacity(256);
    loop {
        if stream.read_buf(&mut buf).await? == 0 {
            break Err(Error::Io(io::ErrorKind::UnexpectedEof.into()));
        }

        let mut view = Bytes::copy_from_slice(&buf);
        match Request::from_buf(&mut view) {
            Ok(req) => break Ok(req),
            Err(Error::NeedMoreData) => {}
            Err(e) => break Err(e),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bytes::Bytes;

    #[test]
    fn test_request_from_buf() {
        {
            let mut buf = Bytes::from_static(&[1, 1, 2, 0, 0, 0, 0, 0x68, 0x6f, 0x67, 0x65, 0]);
            let req = Request::from_buf(&mut buf).unwrap();
            assert_eq!(
                req,
                Request::Connect(SocketAddr::v4(0, 0x0102), "hoge".to_string())
            );
        }
        {
            let mut buf =
                Bytes::from_static(&[2, 1, 2, 0, 0, 0, 255, 0, 0x68, 0x6f, 0x67, 0x65, 0]);
            let req = Request::from_buf(&mut buf).unwrap();
            assert_eq!(
                req,
                Request::Bind(SocketAddr::raw("hoge".to_string(), 0x0102), "".to_string())
            );
        }
        {
            let mut buf = Bytes::from_static(&[1, 2, 3, 4, 5, 6, 7]);
            assert!(matches!(
                Request::from_buf(&mut buf),
                Err(Error::NeedMoreData)
            ));
        }
        {
            let mut buf = Bytes::from_static(&[0, 1, 2, 3, 4, 5, 6, 7]);
            assert!(matches!(
                Request::from_buf(&mut buf),
                Err(Error::Protocol(_))
            ));
        }
    }

    #[test]
    fn test_read_string() {
        let mut buf = Bytes::from_static(b"a");
        assert!(matches!(
            Request::read_string(&mut buf),
            Err(Error::NeedMoreData)
        ));
    }
}
