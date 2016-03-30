//! Implements the `PING` HTTP/2 frame.

use std::io;

use http::StreamId;
use http::frame::{
    Frame,
    FrameIR,
    FrameBuilder,
    FrameHeader,
    RawFrame,
    Flag,
};

/// Ping frames are always 8 bytes
pub const PING_FRAME_LEN: u32 = 8;
/// The frame type of the `PING` frame.
pub const PING_FRAME_TYPE: u8 = 0x6;

pub struct PingFlag;
impl Flag for PingFlag {
    fn bitmask(&self) -> u8 {
        0x1
    }
}

/// The struct represents the `PINg` HTTP/2 frame.
#[derive(Clone, Debug, PartialEq)]
pub struct PingFrame {
    opaque_data: u64,
    flags: u8,
}

impl PingFrame {
    /// Create a new `PING` frame
    pub fn new() -> Self {
        PingFrame {
            opaque_data: 0,
            flags: 0,
        }
    }

    /// Create a new PING frame with ACK set
    pub fn new_ack(opaque_data: u64) -> Self {
        PingFrame {
            opaque_data: opaque_data,
            flags: 1,
        }
    }

    /// Create a new `PING` frame with the given opaque_data
    pub fn with_data(opaque_data: u64) -> Self {
        PingFrame {
            opaque_data: opaque_data,
            flags: 0,
        }
    }

    pub fn is_ack(&self) -> bool {
        self.is_set(PingFlag)
    }

    pub fn opaque_data(&self) -> u64 {
        self.opaque_data
    }
}

impl<'a> Frame<'a> for PingFrame {
    type FlagType = PingFlag;

    fn from_raw(raw_frame: &'a RawFrame<'a>) -> Option<Self> {
        let (payload_len, frame_type, flags, stream_id) = raw_frame.header();
        if payload_len != PING_FRAME_LEN {
            return None;
        }
        if frame_type != PING_FRAME_TYPE {
            return None;
        }
        if stream_id != 0x0 {
            return None;
        }

        let data = unpack_octets_4!(raw_frame.payload(), 0, u64) << 32 |
                   unpack_octets_4!(raw_frame.payload(), 4, u64);

        Some(PingFrame {
            opaque_data: data,
            flags: flags,
        })
    }

    fn is_set(&self, flag: PingFlag) -> bool {
        (flag.bitmask() & self.flags) != 0
    }

    fn get_stream_id(&self) -> StreamId {
        0
    }

    fn get_header(&self) -> FrameHeader {
        (PING_FRAME_LEN, PING_FRAME_TYPE, self.flags, 0)
    }
}

impl<'a> FrameIR for PingFrame {
    fn serialize_into<B: FrameBuilder>(self, builder: &mut B) -> io::Result<()> {
        try!(builder.write_header(self.get_header()));
        try!(builder.write_u32((self.opaque_data >> 32) as u32));
        try!(builder.write_u32(self.opaque_data as u32));
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::PingFrame;

    use http::tests::common::{serialize_frame, raw_frame_from_parts};
    use http::frame::Frame;

    #[test]
    fn test_parse_not_ack() {
        let raw = raw_frame_from_parts((8, 0x6, 0, 0), vec![0, 0, 0, 0, 0, 0, 0, 0]);
        let frame = PingFrame::from_raw(&raw).expect("Expected successful parse");
        assert_eq!(frame.is_ack(), false);
        assert_eq!(frame.opaque_data(), 0);
    }

    #[test]
    fn test_parse_ack() {
        let raw = raw_frame_from_parts((8, 0x6, 1, 0), vec![0, 0, 0, 0, 0, 0, 0, 0]);
        let frame = PingFrame::from_raw(&raw).expect("Expected successful parse");
        assert_eq!(frame.is_ack(), true);
        assert_eq!(frame.opaque_data(), 0);
    }

    #[test]
    fn test_parse_opaque_data() {
        let raw = raw_frame_from_parts((8, 0x6, 1, 0), vec![1, 2, 3, 4, 5, 6, 7, 8]);
        let frame = PingFrame::from_raw(&raw).expect("Expected successful parse");
        assert_eq!(frame.is_ack(), true);
        assert_eq!(frame.opaque_data(), 0x0102030405060708);
    }

    #[test]
    fn test_serialize() {
        let frame = PingFrame::new_ack(0);
        let expected: Vec<u8> = raw_frame_from_parts(
            (8, 0x6, 1, 0),
            vec![0, 0, 0, 0, 0, 0, 0, 0]).into();

        let raw = serialize_frame(&frame);

        assert_eq!(expected, raw);
    }
}
