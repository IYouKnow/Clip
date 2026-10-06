//! Keyframe-aware ring buffer of encoded video packets.
//!
//! Holds the last N seconds of *encoded* packets so a clip can be written by
//! remuxing (no re-encode). Video is the only thing buffered as packets: raw
//! frames are far too large to keep in memory.

use std::ops::Range;

use ffmpeg_next as ffmpeg;
use ffmpeg::Packet;

/// Seconds a packet's presentation timestamp represents.
fn seconds_of(packet: &Packet, time_base: ffmpeg::Rational) -> f64 {
    let pts = packet.pts().unwrap_or(0);
    pts as f64 * time_base.0 as f64 / time_base.1 as f64
}

/// A bounded queue of encoded packets, trimmed by duration.
pub struct PacketRing {
    packets: Vec<Packet>,
    time_base: ffmpeg::Rational,
    max_seconds: f64,
}

impl PacketRing {
    pub fn new(time_base: ffmpeg::Rational, max_seconds: f64) -> Self {
        Self {
            packets: Vec::new(),
            time_base,
            max_seconds,
        }
    }

    pub fn push(&mut self, packet: Packet) {
        self.packets.push(packet);
        self.trim();
    }

    /// The time base the packet timestamps are expressed in.
    pub fn time_base(&self) -> ffmpeg::Rational {
        self.time_base
    }

    pub fn len(&self) -> usize {
        self.packets.len()
    }

    pub fn is_empty(&self) -> bool {
        self.packets.is_empty()
    }

    /// Drops packets older than `max_seconds` relative to the newest packet.
    fn trim(&mut self) {
        let Some(newest) = self.packets.last() else {
            return;
        };
        let end = seconds_of(newest, self.time_base);

        while self.packets.len() > 1 {
            let oldest = seconds_of(&self.packets[0], self.time_base);
            if end - oldest > self.max_seconds {
                self.packets.remove(0);
            } else {
                break;
            }
        }
    }

    /// Range of packets covering the last `seconds`, starting on a keyframe.
    pub fn snapshot_range(&self, seconds: f64) -> Range<usize> {
        if self.packets.is_empty() {
            return 0..0;
        }

        let end = seconds_of(self.packets.last().unwrap(), self.time_base);
        let cutoff = end - seconds;

        // Last packet at or before the cutoff, then the next keyframe at or
        // after it — a clip must begin on a keyframe to be decodable.
        let mut start = 0;
        for (index, packet) in self.packets.iter().enumerate() {
            if seconds_of(packet, self.time_base) <= cutoff {
                start = index;
            } else {
                break;
            }
        }

        while start < self.packets.len() && !self.packets[start].is_key() {
            start += 1;
        }
        if start >= self.packets.len() {
            // No keyframe after the cutoff; fall back to the earliest one.
            start = self
                .packets
                .iter()
                .position(|packet| packet.is_key())
                .unwrap_or(0);
        }

        start..self.packets.len()
    }

    pub fn slice_mut(&mut self, range: Range<usize>) -> &mut [Packet] {
        &mut self.packets[range]
    }
}
