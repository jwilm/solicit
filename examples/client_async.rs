extern crate solicit;

use solicit::http::Header;
use solicit::client::Client;
use solicit::http::client::CleartextConnector;
use std::str;

fn main() {
    // Connect to a server that supports HTTP/2
    let connector = CleartextConnector::new("http2bin.org");
    let (done_tx, done_rx) = ::std::sync::mpsc::channel();
    let client = Client::with_connector(connector, 5, move |resp, user: isize, queued, _limit, pending| {
        println!("got response ... {}", resp.status_code().ok().unwrap());
        println!("The response contains the following headers:");
        for header in resp.headers.iter() {
            println!("  {}: {}",
                  str::from_utf8(header.name()).unwrap(),
                  str::from_utf8(header.value()).unwrap());
        }
        println!("Body:");
        println!("{}", str::from_utf8(&resp.body).unwrap());
        println!("User Data: {}", user);
        if pending == 1 && queued == 0 {
            done_tx.send(true).unwrap();
        }
    }).unwrap();

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

    done_rx.recv().unwrap();
}
