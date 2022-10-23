use super::*;
use bytes::{Buf, BufMut, Bytes, BytesMut};
use std::io::Read;
use std::vec;
use tokio::io::AsyncWriteExt;
use tokio::net::TcpStream;

#[derive(Debug, Clone, PartialEq, Eq)]
enum Request {
    Connect(SocketAddr),
    Bind(SocketAddr),
    UdpAssociate(SocketAddr),
}

impl Request {
    fn from_buf<B: Buf>(buf: &mut B) -> Result<Self> {
        if buf.remaining() < 2 {
            return Err(Error::NeedMoreData);
        }

        let cmd = buf.get_u8();
        let rsv = buf.get_u8();
        if rsv != 0 {
            return Err(Error::Protocol("reserved octet is not 0".to_string()));
        }
        if !matches!(cmd, 1 | 2 | 3) {
            return Err(Error::Protocol(format!("illegal request `{cmd}`")));
        }

        if buf.remaining() < 1 {
            return Err(Error::NeedMoreData);
        }
        let addr = match buf.get_u8() {
            1 => {
                if buf.remaining() < 6 {
                    return Err(Error::NeedMoreData);
                }
                SocketAddr::v4(buf.get_u32(), buf.get_u16())
            }
            4 => {
                if buf.remaining() < 18 {
                    return Err(Error::NeedMoreData);
                }
                SocketAddr::v6(buf.get_u128(), buf.get_u16())
            }
            3 => {
                if buf.remaining() < 1 {
                    return Err(Error::NeedMoreData);
                }

                let len = buf.get_u8() as usize;
                if buf.remaining() < len + 2 {
                    return Err(Error::NeedMoreData);
                }

                let mut vec = vec![0; len];
                buf.reader().read_exact(&mut vec)?;

                let domain = String::from_utf8(vec).map_err(|e| Error::Protocol(e.to_string()))?;
                SocketAddr::raw(domain, buf.get_u16())
            }
            a_type => return Err(Error::Protocol(format!("illegal address type `{a_type}`"))),
        };

        match cmd {
            1 => Ok(Request::Connect(addr)),
            2 => Ok(Request::Bind(addr)),
            3 => Ok(Request::UdpAssociate(addr)),
            _ => unreachable!(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Response {
    Succeeded,
    Failed,
    Unsupported,
}

impl From<Response> for Bytes {
    fn from(res: Response) -> Self {
        let code = match res {
            Response::Succeeded => 0,
            Response::Failed => 1,
            Response::Unsupported => 7,
        };

        let mut buf = BytesMut::with_capacity(16);

        buf.put_u8(5);
        buf.put_u8(code);
        buf.put_u8(0);
        buf.put_u8(1);
        buf.put_u32(0);
        buf.put_u16(0);

        buf.freeze()
    }
}

pub async fn handle_request(mut client: TcpStream) -> Result<()> {
    let auth_req = {
        let len = client.read_u8().await? as usize;
        let mut auth = vec![0; len];
        client.read_exact(&mut auth).await?;
        auth
    };
    if auth_req.contains(&0x00) {
        client.write_all(&[0x05, 0x00]).await?;
    } else {
        client.write_all(&[0x05, 0xFF]).await?;
        return Ok(());
    }

    let request = read_request(&mut client).await?;

    let (server, response) = match request {
        Request::Connect(addr) => {
            let res = match addr {
                SocketAddr::V4(addr) => TcpStream::connect(addr).await,
                SocketAddr::V6(addr) => TcpStream::connect(addr).await,
                SocketAddr::Raw(domain, port) => TcpStream::connect((domain, port)).await,
            };

            match res {
                Ok(server) => (Some(server), Response::Succeeded),
                Err(_) => (None, Response::Failed),
            }
        }
        Request::Bind(_) | Request::UdpAssociate(_) => (None, Response::Unsupported),
    };

    let mut buf: Bytes = response.into();
    client.write_all_buf(&mut buf).await?;

    if let Some(mut server) = server {
        io::copy_bidirectional(&mut client, &mut server).await?;
    }

    Ok(())
}

async fn read_request(client: &mut TcpStream) -> Result<Request> {
    let ver = client.read_u8().await?;
    if ver != 5 {
        return Err(Error::Protocol(format!("illegal version number `{ver}`")));
    }

    let mut buf = BytesMut::with_capacity(256);
    loop {
        if client.read_buf(&mut buf).await? == 0 {
            break Err(Error::Io(io::ErrorKind::UnexpectedEof.into()));
        }

        let mut view = Bytes::copy_from_slice(&buf);
        match Request::from_buf(&mut view) {
            Ok(req) => break Ok(req),
            Err(Error::NeedMoreData) => continue,
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
            let mut buf = Bytes::from_static(&[1, 0, 1, 2, 3, 4, 5, 6, 7]);
            let req = Request::from_buf(&mut buf).unwrap();
            assert_eq!(req, Request::Connect(SocketAddr::v4(0x02030405, 0x0607)));
        }
        {
            let mut buf = Bytes::from_static(&[
                2, 0, 4, 0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0A, 0x0B,
                0x0C, 0x0D, 0x0E, 0x0F, 0x10, 0x11,
            ]);
            let req = Request::from_buf(&mut buf).unwrap();
            assert_eq!(
                req,
                Request::Bind(SocketAddr::v6(0x000102030405060708090A0B0C0D0E0F, 0x1011))
            );
        }
        {
            let mut buf = Bytes::from_static(&[3, 0, 3, 4, 0x68, 0x6f, 0x67, 0x65, 0x12, 0x34]);
            let req = Request::from_buf(&mut buf).unwrap();
            assert_eq!(
                req,
                Request::UdpAssociate(SocketAddr::raw("hoge".to_string(), 0x1234))
            );
        }
        {
            let mut buf = Bytes::from_static(&[1]);
            assert!(matches!(
                Request::from_buf(&mut buf),
                Err(Error::NeedMoreData)
            ));
        }
        {
            let mut buf = Bytes::from_static(&[1, 2]);
            assert!(matches!(
                Request::from_buf(&mut buf),
                Err(Error::Protocol(_))
            ));
        }
        {
            let mut buf = Bytes::from_static(&[0, 0]);
            assert!(matches!(
                Request::from_buf(&mut buf),
                Err(Error::Protocol(_))
            ));
        }
        {
            let mut buf = Bytes::from_static(&[3, 0]);
            assert!(matches!(
                Request::from_buf(&mut buf),
                Err(Error::NeedMoreData)
            ));
        }
        {
            let mut buf = Bytes::from_static(&[1, 0, 1, 0, 0, 0]);
            assert!(matches!(
                Request::from_buf(&mut buf),
                Err(Error::NeedMoreData)
            ));
        }
        {
            let mut buf =
                Bytes::from_static(&[2, 0, 4, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
            assert!(matches!(
                Request::from_buf(&mut buf),
                Err(Error::NeedMoreData)
            ));
        }
        {
            let mut buf = Bytes::from_static(&[2, 0, 3]);
            assert!(matches!(
                Request::from_buf(&mut buf),
                Err(Error::NeedMoreData)
            ));
        }
        {
            let mut buf = Bytes::from_static(&[2, 0, 3, 4, 0, 0, 0]);
            assert!(matches!(
                Request::from_buf(&mut buf),
                Err(Error::NeedMoreData)
            ));
        }
        {
            let mut buf = Bytes::from_static(&[2, 0, 5]);
            assert!(matches!(
                Request::from_buf(&mut buf),
                Err(Error::Protocol(_))
            ));
        }
    }
}
