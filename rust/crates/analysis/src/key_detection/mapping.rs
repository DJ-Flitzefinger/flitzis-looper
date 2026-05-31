//! Camelot Wheel index (0–23) to musical key string mapping.
//!
//! Indices 0–11 represent minor keys and 12–23 represent major keys,
//! following the standard Camelot Wheel ordering used in DJ software.

/// Map a Camelot class index (0–23) to a musical key string.
///
/// Uses sharp notation exclusively (e.g., "G#m" instead of "Abm") to
/// match the UI key selector.
///
/// Returns `None` for out-of-range indices.
pub fn camelot_index_to_key(index: usize) -> Option<&'static str> {
    Some(match index {
        // Minor keys (indices 0–11)
        0 => "G#m",
        1 => "D#m",
        2 => "A#m",
        3 => "Fm",
        4 => "Cm",
        5 => "Gm",
        6 => "Dm",
        7 => "Am",
        8 => "Em",
        9 => "Bm",
        10 => "F#m",
        11 => "C#m",
        // Major keys (indices 12–23)
        12 => "B",
        13 => "F#",
        14 => "C#",
        15 => "G#",
        16 => "D#",
        17 => "A#",
        18 => "F",
        19 => "C",
        20 => "G",
        21 => "D",
        22 => "A",
        23 => "E",
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_all_minor_keys() {
        assert_eq!(camelot_index_to_key(0), Some("G#m"));
        assert_eq!(camelot_index_to_key(1), Some("D#m"));
        assert_eq!(camelot_index_to_key(2), Some("A#m"));
        assert_eq!(camelot_index_to_key(3), Some("Fm"));
        assert_eq!(camelot_index_to_key(4), Some("Cm"));
        assert_eq!(camelot_index_to_key(5), Some("Gm"));
        assert_eq!(camelot_index_to_key(6), Some("Dm"));
        assert_eq!(camelot_index_to_key(7), Some("Am"));
        assert_eq!(camelot_index_to_key(8), Some("Em"));
        assert_eq!(camelot_index_to_key(9), Some("Bm"));
        assert_eq!(camelot_index_to_key(10), Some("F#m"));
        assert_eq!(camelot_index_to_key(11), Some("C#m"));
    }

    #[test]
    fn test_all_major_keys() {
        assert_eq!(camelot_index_to_key(12), Some("B"));
        assert_eq!(camelot_index_to_key(13), Some("F#"));
        assert_eq!(camelot_index_to_key(14), Some("C#"));
        assert_eq!(camelot_index_to_key(15), Some("G#"));
        assert_eq!(camelot_index_to_key(16), Some("D#"));
        assert_eq!(camelot_index_to_key(17), Some("A#"));
        assert_eq!(camelot_index_to_key(18), Some("F"));
        assert_eq!(camelot_index_to_key(19), Some("C"));
        assert_eq!(camelot_index_to_key(20), Some("G"));
        assert_eq!(camelot_index_to_key(21), Some("D"));
        assert_eq!(camelot_index_to_key(22), Some("A"));
        assert_eq!(camelot_index_to_key(23), Some("E"));
    }

    #[test]
    fn test_out_of_range() {
        assert_eq!(camelot_index_to_key(24), None);
        assert_eq!(camelot_index_to_key(255), None);
    }

    #[test]
    fn test_spec_examples() {
        // From spec: 7→"Am", 19→"C"
        assert_eq!(camelot_index_to_key(7), Some("Am"));
        assert_eq!(camelot_index_to_key(19), Some("C"));
    }
}
