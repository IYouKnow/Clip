//! Remuxes buffered packets into an MP4 file (stream copy, no re-encode).

use std::path::Path;

use anyhow::Result;
use ffmpeg_next as ffmpeg;
use ffmpeg::Packet;

/// Writes `packets` into a new MP4 at `path`.
pub fn write_mp4(
    path: &Path,
    encoder: &ffmpeg::encoder::video::Encoder,
    in_time_base: ffmpeg::Rational,
    packets: &mut [Packet],
) -> Result<()> {
    let mut output = ffmpeg::format::output(path)?;

    // The stream borrow must end before `write_header`, which sets the muxer's
    // own time base.
    let stream_index = {
        let mut stream = output.add_stream(encoder.id())?;
        stream.set_parameters(encoder);
        stream.index()
    };

    output.write_header()?;

    // Only known after the header is written; rescaling with an unset (0/0)
    // time base would blank the timestamps.
    let out_time_base = output
        .stream(stream_index)
        .map(|stream| stream.time_base())
        .unwrap_or(in_time_base);
    let rescalable = out_time_base.0 != 0 && out_time_base.1 != 0;

    for packet in packets.iter_mut() {
        packet.set_stream(stream_index);
        if rescalable {
            packet.rescale_ts(in_time_base, out_time_base);
        }
        packet.write_interleaved(&mut output)?;
    }

    output.write_trailer()?;
    Ok(())
}
