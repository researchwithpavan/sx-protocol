use rand::{rngs::StdRng, Rng, SeedableRng};
use std::collections::BTreeMap;
use sx_core::SxValue;

#[test]
fn deterministic_random_roundtrip() {
    let mut rng = StdRng::seed_from_u64(7);
    for _ in 0..128 {
        let v = random_value(&mut rng, 0);
        let b = sx_binary::encode_binary(&v, None, None).unwrap();
        let back = sx_binary::decode_binary(&b).unwrap();
        assert_eq!(v, back);
    }
}

fn random_value(rng: &mut StdRng, depth: usize) -> SxValue {
    if depth > 3 {
        return SxValue::I64(rng.gen_range(-1000..1000));
    }
    match rng.gen_range(0..8) {
        0 => SxValue::Null,
        1 => SxValue::Bool(rng.gen_bool(0.5)),
        2 => SxValue::I64(rng.gen_range(-1000..1000)),
        3 => SxValue::U64(rng.gen_range(0..1000)),
        4 => SxValue::F64(rng.gen_range(-100.0..100.0)),
        5 => SxValue::String(format!("s{}", rng.gen::<u32>())),
        6 => {
            let mut arr = Vec::new();
            for _ in 0..rng.gen_range(0..4) {
                arr.push(random_value(rng, depth + 1));
            }
            SxValue::Array(arr)
        }
        _ => {
            let mut obj = BTreeMap::new();
            for i in 0..rng.gen_range(0..4) {
                obj.insert(format!("k{}", i), random_value(rng, depth + 1));
            }
            SxValue::Object(obj)
        }
    }
}
