//! An HTTP endpoint that ANSWERS — a `200` status line and headers — and then
//! breaks its body off short of the `Content-Length` it promised: a foreign
//! service resetting mid-reply. The endpoint was reachable, so a client must
//! not report it as one that never answered.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;

/// The body bytes the reply promises; it sends far fewer.
const PROMISED_BODY_BYTES: usize = 4096;

/// Serve ONE request on an ephemeral port, answer it with a body broken off
/// mid-way, and return the endpoint's URL. The responder thread exits after
/// that request; nothing needs stopping.
#[allow(dead_code)]
pub fn spawn_truncated_reply() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind ephemeral port");
    let url = format!("http://{}/mcp", listener.local_addr().expect("local addr"));
    std::thread::spawn(move || {
        let Ok((stream, _)) = listener.accept() else {
            return;
        };
        let mut reader = BufReader::new(stream);
        let request_body_bytes = read_request_head(&mut reader);
        let mut request_body = vec![0; request_body_bytes];
        let _ = reader.read_exact(&mut request_body);
        let mut stream = reader.into_inner();
        let _ = write!(
            stream,
            "HTTP/1.1 200 OK\r\n\
             Content-Type: application/json\r\n\
             Mcp-Session-Id: truncated-reply\r\n\
             Content-Length: {PROMISED_BODY_BYTES}\r\n\
             \r\n\
             {{\"jsonrpc\":\"2.0\",\"id\":1,\"res"
        );
        let _ = stream.flush();
    });
    url
}

/// Consume the request line and headers; return the request's
/// `Content-Length`, so its body can be read before the reply is written.
fn read_request_head(reader: &mut impl BufRead) -> usize {
    let mut content_length = 0;
    let mut line = String::new();
    loop {
        line.clear();
        match reader.read_line(&mut line) {
            Ok(0) | Err(_) => return content_length,
            Ok(_) if line == "\r\n" => return content_length,
            Ok(_) => {
                if let Some((name, value)) = line.split_once(':')
                    && name.eq_ignore_ascii_case("content-length")
                {
                    content_length = value.trim().parse().unwrap_or(0);
                }
            }
        }
    }
}
