#![no_main]

use libfuzzer_sys::fuzz_target;
use ptpip::{Packet, PacketDecoder};

const MAX_FRAME: u32 = 1024 * 1024;

fuzz_target!(|data: &[u8]| {
    if let Ok(packet) = Packet::decode(data) {
        let encoded = packet.encode().expect("decoded packets must encode");
        let decoded = Packet::decode(&encoded).expect("encoded packets must decode");
        assert_eq!(decoded, packet);
    }

    let chunk_size = data.first().map_or(1, |value| usize::from(*value).max(1));
    let mut decoder = PacketDecoder::with_maximum(MAX_FRAME).expect("valid fuzz limit");
    for chunk in data.chunks(chunk_size) {
        decoder.push(chunk);
        loop {
            match decoder.next_packet() {
                Ok(Some(packet)) => {
                    let encoded = packet.encode().expect("decoded packets must encode");
                    assert_eq!(Packet::decode(&encoded).ok(), Some(packet));
                }
                Ok(None) => break,
                Err(_) => break,
            }
        }
    }
});
