//! The module contains a number of reusable components for implementing the client side of an
//! HTTP/2 connection.

use http::{HttpScheme, HttpResult, Request, StreamId, Header};
use http::connection::{
    SendFrame, ReceiveFrame,
    HttpConnection,
};
use http::session::{Session, Stream, DefaultStream, DefaultSessionState, SessionState};

/// The struct extends the `HttpConnection` API with client-specific methods (such as
/// `send_request`) and wires the `HttpConnection` to the client `Session` callbacks.
pub struct ClientConnection<S, R, Sess>
        where S: SendFrame, R: ReceiveFrame, Sess: Session {
    /// The underlying `HttpConnection` that will be used for any HTTP/2
    /// communication.
    conn: HttpConnection<S, R>,
    /// The `Session` associated with this connection. It is essentially a set
    /// of callbacks that are triggered by the connection when different states
    /// in the HTTP/2 communication arise.
    pub session: Sess,
}

impl<S, R, Sess> ClientConnection<S, R, Sess> where S: SendFrame, R: ReceiveFrame, Sess: Session {
    /// Creates a new `ClientConnection` that will use the given `HttpConnection`
    /// for all its underlying HTTP/2 communication.
    ///
    /// The given `session` instance will receive all events that arise from reading frames from
    /// the underlying HTTP/2 connection.
    pub fn with_connection(conn: HttpConnection<S, R>, session: Sess)
            -> ClientConnection<S, R, Sess> {
        ClientConnection {
            conn: conn,
            session: session,
        }
    }

    /// Returns the scheme of the underlying `HttpConnection`.
    #[inline]
    pub fn scheme(&self) -> HttpScheme {
        self.conn.scheme
    }

    /// Performs the initialization of the `ClientConnection`.
    ///
    /// This means that it expects the next frame that it receives to be the server preface -- i.e.
    /// a `SETTINGS` frame. Returns an `HttpError` if this is not the case.
    pub fn init(&mut self) -> HttpResult<()> {
        try!(self.read_preface());
        Ok(())
    }

    /// Reads and handles the server preface from the underlying HTTP/2
    /// connection.
    ///
    /// According to the HTTP/2 spec, a server preface consists of a single
    /// settings frame.
    ///
    /// # Returns
    ///
    /// Any error raised by the underlying connection is propagated.
    ///
    /// Additionally, if it is not possible to decode the server preface,
    /// it returns the `HttpError::UnableToConnect` variant.
    fn read_preface(&mut self) -> HttpResult<()> {
        self.conn.expect_settings(&mut self.session)
    }

    /// A method that sends the given `Request` to the server.
    ///
    /// The method blocks until the entire request has been sent.
    ///
    /// All errors are propagated.
    pub fn send_request(&mut self, req: Request) -> HttpResult<()> {
        let end_of_stream = req.body.len() == 0;
        try!(self.conn.send_headers(req.headers, req.stream_id, end_of_stream));
        if !end_of_stream {
            // Queue the entire request body for transfer now...
            // Also assumes that the entire body fits into a single frame.
            // TODO Stash the body locally (associated to a stream) and send it out depending on a
            //      pluggable stream prioritization strategy.
            try!(self.conn.send_data(req.body, req.stream_id, true));
        }

        Ok(())
    }

    /// Fully handles the next incoming frame. Events are passed on to the internal `session`
    /// instance.
    #[inline]
    pub fn handle_next_frame(&mut self) -> HttpResult<()> {
        self.conn.handle_next_frame(&mut self.session)
    }
}

/// A simple implementation of the `Session` trait.
///
/// Relies on the `DefaultSessionState` to keep track of its currently open streams.
///
/// The purpose of the type is to make it easier for client implementations to
/// only handle stream-level events by providing a `Stream` implementation,
/// instead of having to implement the entire session management (tracking active
/// streams, etc.).
///
/// For example, by varying the `Stream` implementation it is easy to implement
/// a client that streams responses directly into a file on the local file system,
/// instead of keeping it in memory (like the `DefaultStream` does), without
/// having to change any HTTP/2-specific logic.
pub struct ClientSession<S=DefaultStream> where S: Stream {
    state: DefaultSessionState<S>,
}

impl<S> ClientSession<S> where S: Stream {
    /// Returns a new `ClientSession` with no active streams.
    pub fn new() -> ClientSession<S> {
        ClientSession {
            state: DefaultSessionState::new(),
        }
    }

    /// Returns a reference to a stream with the given ID, if such a stream is
    /// found in the `ClientSession`.
    #[inline]
    pub fn get_stream(&self, stream_id: StreamId) -> Option<&S> {
        self.state.get_stream_ref(stream_id)
    }

    #[inline]
    pub fn get_stream_mut(&mut self, stream_id: StreamId) -> Option<&mut S> {
        self.state.get_stream_mut(stream_id)
    }

    /// Creates a new stream with the given ID in the session.
    #[inline]
    pub fn new_stream(&mut self, stream_id: StreamId) {
        self.state.insert_stream(Stream::new(stream_id));
    }

    /// Returns all streams that are closed and tracked by the session.
    ///
    /// The streams are moved out of the session.
    #[inline]
    pub fn get_closed(&mut self) -> Vec<S> {
        self.state.get_closed()
    }
}

impl<S> Session for ClientSession<S> where S: Stream {
    fn new_data_chunk(&mut self, stream_id: StreamId, data: &[u8]) {
        debug!("Data chunk for stream {}", stream_id);
        let mut stream = match self.state.get_stream_mut(stream_id) {
            None => {
                debug!("Received a frame for an unknown stream!");
                return;
            },
            Some(stream) => stream,
        };
        // Now let the stream handle the data chunk
        stream.new_data_chunk(data);
    }

    fn new_headers(&mut self, stream_id: StreamId, headers: Vec<Header>) {
        debug!("Headers for stream {}", stream_id);
        let mut stream = match self.state.get_stream_mut(stream_id) {
            None => {
                debug!("Received a frame for an unknown stream!");
                return;
            },
            Some(stream) => stream,
        };
        // Now let the stream handle the headers
        stream.set_headers(headers);
    }

    fn end_of_stream(&mut self, stream_id: StreamId) {
        debug!("End of stream {}", stream_id);
        let mut stream = match self.state.get_stream_mut(stream_id) {
            None => {
                debug!("Received a frame for an unknown stream!");
                return;
            },
            Some(stream) => stream,
        };
        stream.close()
    }
}


#[cfg(test)]
mod tests {
    use super::{
        ClientConnection,
        ClientSession,
    };

    use http::Request;
    use http::tests::common::{
        TestSession,
        build_mock_http_conn,
    };
    use http::frame::{
        SettingsFrame,
        DataFrame,
    };
    use http::connection::{
        HttpFrame,
    };
    use http::session::{Session, SessionState, Stream};

    /// Tests that a client connection is correctly initialized, by reading the
    /// server preface (i.e. a settings frame) as the first frame of the connection.
    #[test]
    fn test_init_client_conn() {
        let frames = vec![HttpFrame::SettingsFrame(SettingsFrame::new())];
        let mut conn = ClientConnection::with_connection(
            build_mock_http_conn(frames),
            TestSession::new());

        conn.init().unwrap();

        // We have read the server's response (the settings frame only, since no panic
        // ocurred)
        assert_eq!(conn.conn.receiver.recv_list.len(), 0);
        // We also sent an ACK already.
        let frame = match conn.conn.sender.sent.remove(0) {
            HttpFrame::SettingsFrame(frame) => frame,
            _ => panic!("ACK not sent!"),
        };
        assert!(frame.is_ack());
    }

    /// Tests that a client connection fails to initialize when the server does
    /// not send a settings frame as its first frame (i.e. server preface).
    #[test]
    fn test_init_client_conn_no_settings() {
        let frames = vec![HttpFrame::DataFrame(DataFrame::new(1))];
        let mut conn = ClientConnection::with_connection(
            build_mock_http_conn(frames),
            TestSession::new());

        // We get an error since the first frame sent by the server was not
        // SETTINGS.
        assert!(conn.init().is_err());
    }

    /// Tests that a `ClientConnection` correctly sends a `Request` with no
    /// body.
    #[test]
    fn test_client_conn_send_request_no_body() {
        let req = Request {
            stream_id: 1,
            // An incomplete header list, but this does not matter for this test.
            headers: vec![
                (b":method".to_vec(), b"GET".to_vec()),
                (b":path".to_vec(), b"/".to_vec()),
             ],
            body: Vec::new(),
        };
        let mut conn = ClientConnection::with_connection(
            build_mock_http_conn(vec![]), TestSession::new());

        conn.send_request(req).unwrap();

        let frame = match conn.conn.sender.sent.remove(0) {
            HttpFrame::HeadersFrame(frame) => frame,
            _ => panic!("Headers not sent!"),
        };
        // We sent a headers frame with end of headers and end of stream flags
        assert!(frame.is_headers_end());
        assert!(frame.is_end_of_stream());
        // ...and nothing else!
        assert_eq!(conn.conn.sender.sent.len(), 0);
    }

    /// Tests that a `ClientConnection` correctly sends a `Request` with a small body (i.e. a body
    /// that fits into a single HTTP/2 DATA frame).
    #[test]
    fn test_client_conn_send_request_with_small_body() {
        let body = vec![1, 2, 3];
        let req = Request {
            stream_id: 1,
            // An incomplete header list, but this does not matter for this test.
            headers: vec![
                (b":method".to_vec(), b"GET".to_vec()),
                (b":path".to_vec(), b"/".to_vec()),
             ],
            body: body.clone(),
        };
        let mut conn = ClientConnection::with_connection(
            build_mock_http_conn(vec![]), TestSession::new());

        conn.send_request(req).unwrap();

        let frame = match conn.conn.sender.sent.remove(0) {
            HttpFrame::HeadersFrame(frame) => frame,
            _ => panic!("Headers not sent!"),
        };
        // The headers were sent, but didn't close the stream
        assert!(frame.is_headers_end());
        assert!(!frame.is_end_of_stream());
        // A single data frame is found that *did* close the stream
        let frame = match conn.conn.sender.sent.remove(0) {
            HttpFrame::DataFrame(frame) => frame,
            _ => panic!("Headers not sent!"),
        };
        assert!(frame.is_end_of_stream());
        // The data bore the correct payload
        assert_eq!(frame.data, body);
        // ...and nothing else was sent!
        assert_eq!(conn.conn.sender.sent.len(), 0);
    }

    /// Tests that a `ClientSession` notifies the correct stream when the
    /// appropriate callback is invoked.
    ///
    /// A better unit test would give a mock Stream to the `ClientSession`,
    /// instead of testing both the `ClientSession` and the `DefaultStream`
    /// in the same time...
    #[test]
    fn test_client_session_notifies_stream() {
        let mut session: ClientSession = ClientSession::new();
        session.new_stream(1);

        // Registering some data to stream 1...
        session.new_data_chunk(1, &[1, 2, 3]);
        // ...works.
        assert_eq!(session.get_stream(1).unwrap().body, vec![1, 2, 3]);
        // Some more...
        session.new_data_chunk(1, &[4]);
        // ...works.
        assert_eq!(session.get_stream(1).unwrap().body, vec![1, 2, 3, 4]);
        // Now headers?
        let headers = vec![(b":method".to_vec(), b"GET".to_vec())];
        session.new_headers(1, headers.clone());
        assert_eq!(session.get_stream(1).unwrap().headers.clone().unwrap(),
                   headers);
        // Add another stream in the mix
        session.new_stream(3);
        // and send it some data
        session.new_data_chunk(3, &[100]);
        assert_eq!(session.get_stream(3).unwrap().body, vec![100]);
        // Finally, the stream 1 ends...
        session.end_of_stream(1);
        // ...and gets closed.
        assert!(session.get_stream(1).unwrap().closed);
        // but not the other one.
        assert!(!session.get_stream(3).unwrap().closed);
        // Sanity check: both streams still found in the session
        assert_eq!(session.state.iter().collect::<Vec<_>>().len(), 2);
        // The closed stream is returned...
        let closed = session.get_closed();
        assert_eq!(closed.len(), 1);
        assert_eq!(closed[0].id(), 1);
        // ...and is also removed from the session!
        assert_eq!(session.state.iter().collect::<Vec<_>>().len(), 1);
    }
}