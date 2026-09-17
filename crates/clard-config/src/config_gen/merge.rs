//! 深合并（doc/01 §8）：Map 递归合并、Scalar 后者覆盖、List 整体替换。

use serde_yaml_ng::Value;

/// 将 `overlay` 合并到 `base`：
/// - 双方都是 Mapping → 键级递归合并；
/// - 其余（Scalar / Sequence / 只有一方是 Mapping）→ 后者整体替换。
pub fn deep_merge(base: &Value, overlay: &Value) -> Value {
    match (base, overlay) {
        (Value::Mapping(a), Value::Mapping(b)) => {
            let mut out = a.clone();
            for (k, v) in b {
                match out.get(k) {
                    Some(existing) => {
                        out.insert(k.clone(), deep_merge(existing, v));
                    }
                    None => {
                        out.insert(k.clone(), v.clone());
                    }
                }
            }
            Value::Mapping(out)
        }
        _ => overlay.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(s: &str) -> Value {
        serde_yaml_ng::from_str(s).unwrap()
    }

    #[test]
    fn map_merges_recursively() {
        let base = parse("a:\n  x: 1\n  y: 2\nb: 1\n");
        let over = parse("a:\n  y: 3\n  z: 4\n");
        let out = deep_merge(&base, &over);
        let m = out.as_mapping().unwrap();
        let a = m.get(Value::String("a".into())).unwrap().as_mapping().unwrap();
        assert_eq!(a.len(), 3, "x 保留、y 覆盖、z 新增");
        assert_eq!(m.get(Value::String("b".into())).unwrap().as_i64(), Some(1));
    }

    #[test]
    fn scalar_overridden_by_overlay() {
        let out = deep_merge(&parse("port: 1\n"), &parse("port: 2\n"));
        assert_eq!(
            out.as_mapping()
                .unwrap()
                .get(Value::String("port".into()))
                .unwrap()
                .as_i64(),
            Some(2)
        );
    }

    #[test]
    fn list_replaced_entirely() {
        let out = deep_merge(&parse("dns-hijack: [a]\n"), &parse("dns-hijack: [b, c]\n"));
        let seq = out
            .as_mapping()
            .unwrap()
            .get(Value::String("dns-hijack".into()))
            .unwrap()
            .as_sequence()
            .unwrap();
        assert_eq!(seq.len(), 2);
    }

    #[test]
    fn overlay_wins_when_base_not_mapping() {
        let out = deep_merge(&parse("1\n"), &parse("proxies: []\n"));
        assert!(out.is_mapping());
    }
}
