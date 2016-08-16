extern crate solicit;

use std::sync::mpsc;
use std::str;

use solicit::http::{Header, StaticResponse};
use solicit::client::{Client, ClientDelegate, ClientDoneState};
use solicit::http::client::CleartextConnector;

struct Delegate(mpsc::Sender<StaticResponse>);

impl ClientDelegate<isize> for Delegate {
    fn response(&mut self, resp: StaticResponse, user_data: isize) {
        println!("got response ... {}", resp.status_code().ok().unwrap());
        println!("The response contains the following headers:");
        for header in resp.headers.iter() {
            println!("  {}: {}",
                  str::from_utf8(header.name()).unwrap(),
                  str::from_utf8(header.value()).unwrap());
        }
        println!("Body:");
        println!("{}", str::from_utf8(&resp.body).unwrap());
        println!("User Data: {}", user_data);
        self.0.send(resp).unwrap();
    }

    fn halted(&mut self, _: ClientDoneState<isize>) {
        // pass
    }
}

fn main() {
    // Connect to a server that supports HTTP/2
    let connector = CleartextConnector::new("http2bin.org");
    let (done_tx, done_rx) = ::std::sync::mpsc::channel();
    let delegate = Delegate(done_tx);
    let client = Client::with_connector(connector, 5, delegate).unwrap();

    // Issue 5 requests from 5 different threads concurrently and wait for all
    // threads to receive their response.
    for i in 0..5 {
        client.get(b"/get", &[
            // A fully static header
            Header::new(&b"x-solicit"[..], &b"Hello"[..]),
            // A header with a static name, but dynamically allocated value
            Header::new(&b"x-solicit"[..], vec![b'0' + i as u8]),
        ], i as isize).unwrap();
    }

    let mut received = 0;
    while let Ok(_) = done_rx.recv() {
        received += 1;
        if received == 5 {
            break;
        }
    }
}
