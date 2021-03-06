//! The module contains a simple HTTP/2 server implementation.

use http::{Response, StaticResponse, HttpResult, HttpError, HttpScheme, StreamId, Header};
use http::transport::{TransportStream, TransportReceiveFrame};
use http::connection::{HttpConnection, EndStream, SendStatus};
use http::session::{
    DefaultSessionState,
    SessionState,
    Stream,
    DefaultStream,
};
use http::session::Server as ServerMarker;
use http::server::{ServerConnection, StreamFactory};

/// The struct represents a fully received request.
pub struct ServerRequest<'a, 'n, 'v> where 'n: 'a, 'v: 'a {
    pub stream_id: StreamId,
    pub headers: &'a [Header<'n, 'v>],
    pub body: &'a [u8],
}

/// A simple implementation of the `http::server::StreamFactory` trait that creates new
/// `DefaultStream` instances.
struct SimpleFactory;
impl StreamFactory for SimpleFactory {
    type Stream = DefaultStream;
    fn create(&mut self, id: StreamId) -> DefaultStream {
        DefaultStream::with_id(id)
    }
}

/// The struct implements a simple HTTP/2 server that allows users to register a request handler (a
/// callback taking a `ServerRequest` and returning a `Response`) which is run on all received
/// requests.
///
/// The `handle_next` method needs to be called regularly in order to have the server process
/// received frames, as well as send out the responses.
///
/// This is an exceedingly simple implementation of an HTTP/2 server and is mostly an example of
/// how the `solicit::http` API can be used to make one.
///
/// # Examples
///
/// ```no_run
/// extern crate solicit;
/// use std::str;
/// use std::net::{TcpListener, TcpStream};
/// use std::thread;
///
/// use solicit::server::SimpleServer;
///
/// use solicit::http::{Response, Header};
///
/// fn main() {
///     fn handle_client(stream: TcpStream) {
///         let mut server = SimpleServer::new(stream, |req| {
///             println!("Received request:");
///             for header in req.headers.iter() {
///                 println!("  {}: {}",
///                 str::from_utf8(header.name()).unwrap(),
///                 str::from_utf8(header.value()).unwrap());
///             }
///             println!("Body:\n{}", str::from_utf8(&req.body).unwrap());
///
///             // Return a dummy response for every request
///             Response {
///                 headers: vec![
///                     Header::new(b":status", b"200"),
///                     Header::new(b"x-solicit".to_vec(), b"Hello, World!".to_vec()),
///                 ],
///                 body: vec![65],
///                 stream_id: req.stream_id,
///            }
///         }).unwrap();
///         while let Ok(_) = server.handle_next() {}
///         println!("Server done (client disconnected)");
///     }
///
///     let listener = TcpListener::bind("127.0.0.1:8080").unwrap();
///     for stream in listener.incoming() {
///         let stream = stream.unwrap();
///         thread::spawn(move || {
///             handle_client(stream)
///         });
///     }
/// }
/// ```
pub struct SimpleServer<TS, H>
        where TS: TransportStream,
              H: FnMut(ServerRequest) -> Response<'static, 'static> {
    conn: ServerConnection<SimpleFactory>,
    receiver: TS,
    sender: TS,
    handler: H,
}

impl<TS, H> SimpleServer<TS, H>
        where TS: TransportStream, H: FnMut(ServerRequest) -> Response<'static, 'static> {
    /// Creates a new `SimpleServer` that will use the given `TransportStream` to communicate to
    /// the client. Assumes that the stream is fully uninitialized -- no preface sent or read yet.
    pub fn new(mut stream: TS, handler: H) -> HttpResult<SimpleServer<TS, H>> {
        // First assert that the preface is received
        let mut preface = [0; 24];
        TransportStream::read_exact(&mut stream, &mut preface).unwrap();
        if &preface != b"PRI * HTTP/2.0\r\n\r\nSM\r\n\r\n" {
            return Err(HttpError::UnableToConnect);
        }

        let conn = HttpConnection::new(HttpScheme::Http);
        let state = DefaultSessionState::<ServerMarker, _>::new();
        let conn = ServerConnection::with_connection(conn, state, SimpleFactory);
        let mut server = SimpleServer {
            conn: conn,
            receiver: try!(stream.try_split()),
            sender: stream,
            handler: handler,
        };

        // Initialize the connection -- send own settings and process the peer's
        try!(server.conn.send_settings(&mut server.sender));
        try!(server.conn.expect_settings(
            &mut TransportReceiveFrame::new(&mut server.receiver),
            &mut server.sender));

        // Set up done
        Ok(server)
    }

    /// Handles the next incoming frame, blocking to receive it if nothing is available on the
    /// underlying stream.
    ///
    /// Handling the frame can trigger the handler callback. Any responses returned by the handler
    /// are immediately flushed out to the client (blocking the call until it's done).
    pub fn handle_next(&mut self) -> HttpResult<()> {
        try!(self.conn.handle_next_frame(
            &mut TransportReceiveFrame::new(&mut self.receiver),
            &mut self.sender));
        let responses = try!(self.handle_requests());
        try!(self.prepare_responses(responses));
        try!(self.flush_streams());
        try!(self.reap_streams());

        Ok(())
    }

    /// Invokes the request handler for each fully received request. Collects all the responses
    /// into the returned `Vec`.
    fn handle_requests(&mut self) -> HttpResult<Vec<StaticResponse>> {
        let handler = &mut self.handler;
        let closed = self.conn.state.iter()
                       .filter(|&(_, ref s)| s.is_closed_remote());
        let responses = closed.map(|(&stream_id, stream)| {
            let req = ServerRequest {
                stream_id: stream_id,
                headers: stream.headers.as_ref().unwrap(),
                body: &stream.body,
            };
            handler(req)
        });

        Ok(responses.collect())
    }

    /// Prepares the streams for each of the given responses. Headers for each response are
    /// immediately sent and the data staged into the streams' outgoing buffer.
    fn prepare_responses(&mut self, responses: Vec<Response>) -> HttpResult<()> {
        for response in responses.into_iter() {
            try!(self.conn.start_response(
                    response.headers,
                    response.stream_id,
                    EndStream::No,
                    &mut self.sender));
            let mut stream = self.conn.state.get_stream_mut(response.stream_id).unwrap();
            stream.set_full_data(response.body);
        }

        Ok(())
    }

    /// Flushes the outgoing buffers of all streams.
    #[inline]
    fn flush_streams(&mut self) -> HttpResult<()> {
        while let SendStatus::Sent = try!(self.conn.send_next_data(&mut self.sender)) {}

        Ok(())
    }

    /// Removes closed streams from the connection state.
    #[inline]
    fn reap_streams(&mut self) -> HttpResult<()> {
        // Moves the streams out of the state and then drops them
        let _ = self.conn.state.get_closed();
        Ok(())
    }
}
