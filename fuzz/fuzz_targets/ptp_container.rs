#![no_main]

use libfuzzer_sys::fuzz_target;
use ptp_codec::{CodecLimits, Container, ContainerDecoder};

const MAX_FRAME: u32 = 1024 * 1024;

fuzz_target!(|data: &[u8]| {
    let limits = CodecLimits {
        max_container_length: MAX_FRAME,
    };

    if let Ok(container) = Container::decode_with_limits(data, limits) {
        let encoded = container.encode().expect("decoded containers must encode");
        let decoded = Container::decode_with_limits(&encoded, limits)
            .expect("encoded containers must decode");
        assert_eq!(decoded, container);
    }

    let chunk_size = data.first().map_or(1, |value| usize::from(*value).max(1));
    let mut decoder = ContainerDecoder::new(limits);
    for chunk in data.chunks(chunk_size) {
        decoder.push(chunk);
        loop {
            match decoder.next_container() {
                Ok(Some(container)) => {
                    let encoded = container.encode().expect("decoded containers must encode");
                    assert_eq!(
                        Container::decode_with_limits(&encoded, limits).ok(),
                        Some(container)
                    );
                }
                Ok(None) => break,
                Err(_) => {
                    decoder.clear();
                    break;
                }
            }
        }
    }
});
