//! Node names out of a glTF binary (`.glb`), so a model's layer list comes from the file.
//!
//! GLB layout: a 12-byte header (`glTF`, version, length), then chunks of (length, type, data).
//! The first chunk is JSON.

#[derive(Clone, Debug, PartialEq)]
pub struct GlbInfo {
    /// `nodes[].name` in file order; an unnamed node is reported as `node_<index>`.
    pub nodes: Vec<String>,
    pub meshes: usize,
    pub materials: usize,
}

fn u32_at(b: &[u8], i: usize) -> Option<u32> {
    Some(u32::from_le_bytes([*b.get(i)?, *b.get(i + 1)?, *b.get(i + 2)?, *b.get(i + 3)?]))
}

/// The JSON chunk of a `.glb`.
pub fn json_chunk(bytes: &[u8]) -> Result<&[u8], String> {
    if bytes.len() < 20 || &bytes[0..4] != b"glTF" {
        return Err("not a .glb file: the glTF header is missing".into());
    }
    let version = u32_at(bytes, 4).unwrap_or(0);
    if version != 2 {
        return Err(format!("glTF version {version} is not supported; export the model as glTF 2.0 binary"));
    }
    let len = u32_at(bytes, 12).unwrap_or(0) as usize;
    if u32_at(bytes, 16) != Some(0x4E4F_534A) {
        return Err("the first chunk of the .glb is not JSON".into());
    }
    let end = 20usize.checked_add(len).filter(|e| *e <= bytes.len()).ok_or("the .glb is cut short")?;
    Ok(&bytes[20..end])
}

pub fn info(bytes: &[u8]) -> Result<GlbInfo, String> {
    let json = json_chunk(bytes)?;
    let doc: serde_json::Value = serde_json::from_slice(json).map_err(|e| format!("the .glb's JSON chunk is unreadable: {e}"))?;
    let nodes = doc
        .get("nodes")
        .and_then(|n| n.as_array())
        .map(|arr| {
            arr.iter()
                .take(10_000)
                .enumerate()
                .map(|(i, n)| n.get("name").and_then(|v| v.as_str()).map(String::from).unwrap_or_else(|| format!("node_{i}")))
                .collect()
        })
        .unwrap_or_default();
    let count = |key: &str| doc.get(key).and_then(|v| v.as_array()).map(|a| a.len()).unwrap_or(0);
    Ok(GlbInfo { nodes, meshes: count("meshes"), materials: count("materials") })
}

/// The layer list of a model: its node names, each once, in file order.
pub fn layers(bytes: &[u8]) -> Result<Vec<String>, String> {
    let mut out: Vec<String> = vec![];
    for n in info(bytes)?.nodes {
        if !out.contains(&n) {
            out.push(n);
        }
    }
    Ok(out)
}

/// A minimal valid GLB from a glTF JSON document and a binary buffer, for tests.
pub fn build(json: &serde_json::Value, bin: &[u8]) -> Vec<u8> {
    let mut json_bytes = serde_json::to_vec(json).unwrap_or_default();
    while !json_bytes.len().is_multiple_of(4) {
        json_bytes.push(b' ');
    }
    let mut bin_bytes = bin.to_vec();
    while !bin_bytes.len().is_multiple_of(4) {
        bin_bytes.push(0);
    }
    let total = 12 + 8 + json_bytes.len() + if bin.is_empty() { 0 } else { 8 + bin_bytes.len() };
    let mut out = Vec::with_capacity(total);
    out.extend_from_slice(b"glTF");
    out.extend_from_slice(&2u32.to_le_bytes());
    out.extend_from_slice(&(total as u32).to_le_bytes());
    out.extend_from_slice(&(json_bytes.len() as u32).to_le_bytes());
    out.extend_from_slice(&0x4E4F_534Au32.to_le_bytes());
    out.extend_from_slice(&json_bytes);
    if !bin.is_empty() {
        out.extend_from_slice(&(bin_bytes.len() as u32).to_le_bytes());
        out.extend_from_slice(&0x004E_4942u32.to_le_bytes());
        out.extend_from_slice(&bin_bytes);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn node_names_in_file_order_with_a_name_for_unnamed_nodes() {
        let glb = build(
            &serde_json::json!({"asset": {"version": "2.0"}, "nodes": [{"name": "skin"}, {}, {"name": "bone"}, {"name": "skin"}], "meshes": [{}, {}]}),
            &[],
        );
        let i = info(&glb).expect("reads");
        assert_eq!(i.nodes, vec!["skin", "node_1", "bone", "skin"]);
        assert_eq!(i.meshes, 2);
        assert_eq!(layers(&glb).expect("reads"), vec!["skin", "node_1", "bone"]);
    }

    #[test]
    fn bad_files_say_what_is_wrong() {
        assert!(info(b"hello").unwrap_err().contains("not a .glb"));
        let mut glb = build(&serde_json::json!({}), &[]);
        glb[4] = 1;
        assert!(info(&glb).unwrap_err().contains("version 1"));
        let glb = build(&serde_json::json!({}), &[]);
        assert!(info(&glb[..22]).unwrap_err().contains("cut short"));
    }
}
