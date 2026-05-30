use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;

// ── Transform result ─────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum TransformResult {
    Pass(HashMap<String, String>),
    Drop,
    Error(String),
}

impl TransformResult {
    pub fn pass() -> Self {
        TransformResult::Pass(HashMap::new())
    }

    pub fn pass_with(key: &str, value: &str) -> Self {
        let mut m = HashMap::new();
        m.insert(key.to_string(), value.to_string());
        TransformResult::Pass(m)
    }

    pub fn is_pass(&self) -> bool {
        matches!(self, TransformResult::Pass(_))
    }

    pub fn is_drop(&self) -> bool {
        matches!(self, TransformResult::Drop)
    }
}

// ── Transform trait ──────────────────────────────────────────────────

pub trait Transform: Send + Sync {
    fn name(&self) -> &str;
    fn apply(&self, tile: &mut TileData) -> TransformResult;
    fn box_clone(&self) -> Box<dyn Transform>;
}

impl Clone for Box<dyn Transform> {
    fn clone(&self) -> Self {
        self.box_clone()
    }
}

// ── Tile data (simplified tile for transform operations) ─────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TileData {
    pub id: Uuid,
    pub sensor_id: String,
    pub value: f64,
    pub confidence: f64,
    pub timestamp: u64,
    pub layer: u8,
    pub tags: HashMap<String, String>,
}

impl TileData {
    pub fn new(sensor_id: &str, value: f64) -> Self {
        TileData {
            id: Uuid::new_v4(),
            sensor_id: sensor_id.to_string(),
            value,
            confidence: 1.0,
            timestamp: now_millis(),
            layer: 0,
            tags: HashMap::new(),
        }
    }

    pub fn with_confidence(mut self, c: f64) -> Self {
        self.confidence = c;
        self
    }

    pub fn with_layer(mut self, l: u8) -> Self {
        self.layer = l;
        self
    }

    pub fn with_tag(mut self, k: &str, v: &str) -> Self {
        self.tags.insert(k.to_string(), v.to_string());
        self
    }

    pub fn with_timestamp(mut self, ts: u64) -> Self {
        self.timestamp = ts;
        self
    }
}

// ── Built-in transforms ──────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScaleTransform {
    pub factor: f64,
    pub offset: f64,
}

impl ScaleTransform {
    pub fn new(factor: f64, offset: f64) -> Self {
        ScaleTransform { factor, offset }
    }

    pub fn scale(factor: f64) -> Self {
        ScaleTransform { factor, offset: 0.0 }
    }
}

impl Transform for ScaleTransform {
    fn name(&self) -> &str {
        "scale"
    }

    fn apply(&self, tile: &mut TileData) -> TransformResult {
        tile.value = tile.value * self.factor + self.offset;
        TransformResult::pass()
    }

    fn box_clone(&self) -> Box<dyn Transform> {
        Box::new(self.clone())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThresholdTransform {
    pub min: f64,
    pub max: f64,
    pub drop_out_of_range: bool,
}

impl ThresholdTransform {
    pub fn new(min: f64, max: f64, drop: bool) -> Self {
        ThresholdTransform {
            min,
            max,
            drop_out_of_range: drop,
        }
    }

    pub fn clamp(min: f64, max: f64) -> Self {
        ThresholdTransform {
            min,
            max,
            drop_out_of_range: false,
        }
    }

    pub fn drop_outside(min: f64, max: f64) -> Self {
        ThresholdTransform {
            min,
            max,
            drop_out_of_range: true,
        }
    }
}

impl Transform for ThresholdTransform {
    fn name(&self) -> &str {
        "threshold"
    }

    fn apply(&self, tile: &mut TileData) -> TransformResult {
        if tile.value < self.min || tile.value > self.max {
            if self.drop_out_of_range {
                return TransformResult::Drop;
            }
            tile.value = tile.value.clamp(self.min, self.max);
        }
        TransformResult::pass()
    }

    fn box_clone(&self) -> Box<dyn Transform> {
        Box::new(self.clone())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NormalizeTransform {
    pub min_val: f64,
    pub max_val: f64,
}

impl NormalizeTransform {
    pub fn new(min_val: f64, max_val: f64) -> Self {
        NormalizeTransform { min_val, max_val }
    }

    pub fn standard() -> Self {
        NormalizeTransform {
            min_val: 0.0,
            max_val: 100.0,
        }
    }
}

impl Transform for NormalizeTransform {
    fn name(&self) -> &str {
        "normalize"
    }

    fn apply(&self, tile: &mut TileData) -> TransformResult {
        let range = self.max_val - self.min_val;
        if range.abs() < f64::EPSILON {
            return TransformResult::Error("normalize: zero range".to_string());
        }
        tile.value = (tile.value - self.min_val) / range;
        TransformResult::pass()
    }

    fn box_clone(&self) -> Box<dyn Transform> {
        Box::new(self.clone())
    }
}

// ── Pipeline ─────────────────────────────────────────────────────────

pub struct TransformPipeline {
    transforms: Vec<Box<dyn Transform>>,
}

impl Default for TransformPipeline {
    fn default() -> Self {
        Self::new()
    }
}

impl TransformPipeline {
    pub fn new() -> Self {
        TransformPipeline {
            transforms: Vec::new(),
        }
    }

    /// Add a transform to the end of the pipeline.
    pub fn chain<T: Transform + 'static>(mut self, t: T) -> Self {
        self.transforms.push(Box::new(t));
        self
    }

    /// Apply all transforms in sequence to a tile.
    /// Returns the final result (first Drop or Error wins).
    pub fn apply(&self, tile: &mut TileData) -> TransformResult {
        for t in &self.transforms {
            match t.apply(tile) {
                TransformResult::Pass(meta) => {
                    for (k, v) in meta {
                        tile.tags.insert(k, v);
                    }
                }
                other => return other,
            }
        }
        TransformResult::pass()
    }

    /// Apply pipeline to a batch of tiles, collecting only passing tiles.
    pub fn apply_batch(&self, tiles: Vec<TileData>) -> Vec<TileData> {
        tiles
            .into_iter()
            .filter_map(|mut t| {
                if self.apply(&mut t).is_pass() {
                    Some(t)
                } else {
                    None
                }
            })
            .collect()
    }

    pub fn len(&self) -> usize {
        self.transforms.len()
    }

    pub fn is_empty(&self) -> bool {
        self.transforms.is_empty()
    }
}

// ── Functional helpers ───────────────────────────────────────────────

/// Apply a map function to tile values.
pub fn map<F>(tiles: &mut [TileData], f: F)
where
    F: Fn(f64) -> f64,
{
    for tile in tiles.iter_mut() {
        tile.value = f(tile.value);
    }
}

/// Filter tiles, keeping those matching the predicate.
pub fn filter(tiles: Vec<TileData>, predicate: impl Fn(&TileData) -> bool) -> Vec<TileData> {
    tiles.into_iter().filter(predicate).collect()
}

/// Reduce tiles to a single value.
pub fn reduce(tiles: &[TileData], init: f64, f: impl Fn(f64, &TileData) -> f64) -> f64 {
    tiles.iter().fold(init, f)
}

// ── Helpers ──────────────────────────────────────────────────────────

fn now_millis() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64
}

// ── Tests ────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tile_data_construction() {
        let t = TileData::new("s1", 42.0).with_confidence(0.9).with_layer(2);
        assert_eq!(t.sensor_id, "s1");
        assert!((t.value - 42.0).abs() < 1e-9);
        assert!((t.confidence - 0.9).abs() < 1e-9);
        assert_eq!(t.layer, 2);
    }

    #[test]
    fn scale_transform() {
        let mut t = TileData::new("s1", 10.0);
        let s = ScaleTransform::new(2.0, 5.0);
        let result = s.apply(&mut t);
        assert!(result.is_pass());
        assert!((t.value - 25.0).abs() < 1e-9);
    }

    #[test]
    fn scale_transform_multiply_only() {
        let mut t = TileData::new("s1", 3.0);
        let s = ScaleTransform::scale(3.0);
        s.apply(&mut t);
        assert!((t.value - 9.0).abs() < 1e-9);
    }

    #[test]
    fn threshold_clamp() {
        let mut t = TileData::new("s1", 150.0);
        let th = ThresholdTransform::clamp(0.0, 100.0);
        let result = th.apply(&mut t);
        assert!(result.is_pass());
        assert!((t.value - 100.0).abs() < 1e-9);
    }

    #[test]
    fn threshold_clamp_low() {
        let mut t = TileData::new("s1", -10.0);
        let th = ThresholdTransform::clamp(0.0, 100.0);
        th.apply(&mut t);
        assert!((t.value - 0.0).abs() < 1e-9);
    }

    #[test]
    fn threshold_drop_outside() {
        let mut t = TileData::new("s1", 200.0);
        let th = ThresholdTransform::drop_outside(0.0, 100.0);
        let result = th.apply(&mut t);
        assert!(result.is_drop());
    }

    #[test]
    fn threshold_drop_inside_passes() {
        let mut t = TileData::new("s1", 50.0);
        let th = ThresholdTransform::drop_outside(0.0, 100.0);
        let result = th.apply(&mut t);
        assert!(result.is_pass());
    }

    #[test]
    fn normalize_transform() {
        let mut t = TileData::new("s1", 50.0);
        let n = NormalizeTransform::new(0.0, 100.0);
        n.apply(&mut t);
        assert!((t.value - 0.5).abs() < 1e-9);
    }

    #[test]
    fn normalize_zero_range_error() {
        let mut t = TileData::new("s1", 42.0);
        let n = NormalizeTransform::new(50.0, 50.0);
        let result = n.apply(&mut t);
        assert!(matches!(result, TransformResult::Error(_)));
    }

    #[test]
    fn pipeline_chain_and_apply() {
        let mut t = TileData::new("s1", 200.0);
        let pipeline = TransformPipeline::new()
            .chain(ThresholdTransform::clamp(0.0, 100.0))
            .chain(NormalizeTransform::new(0.0, 100.0));
        let result = pipeline.apply(&mut t);
        assert!(result.is_pass());
        assert!((t.value - 1.0).abs() < 1e-9);
    }

    #[test]
    fn pipeline_drops_on_threshold() {
        let mut t = TileData::new("s1", 200.0);
        let pipeline = TransformPipeline::new()
            .chain(ThresholdTransform::drop_outside(0.0, 100.0));
        let result = pipeline.apply(&mut t);
        assert!(result.is_drop());
    }

    #[test]
    fn pipeline_apply_batch() {
        let tiles = vec![
            TileData::new("s1", 50.0),
            TileData::new("s2", 150.0),
            TileData::new("s3", 75.0),
        ];
        let pipeline = TransformPipeline::new()
            .chain(ThresholdTransform::drop_outside(0.0, 100.0));
        let result = pipeline.apply_batch(tiles);
        assert_eq!(result.len(), 2);
    }

    #[test]
    fn functional_map() {
        let mut tiles = vec![
            TileData::new("s1", 10.0),
            TileData::new("s2", 20.0),
        ];
        map(&mut tiles, |v| v * 2.0);
        assert!((tiles[0].value - 20.0).abs() < 1e-9);
        assert!((tiles[1].value - 40.0).abs() < 1e-9);
    }

    #[test]
    fn functional_filter() {
        let tiles = vec![
            TileData::new("s1", 10.0),
            TileData::new("s2", 50.0),
            TileData::new("s3", 90.0),
        ];
        let filtered = filter(tiles, |t| t.value > 40.0);
        assert_eq!(filtered.len(), 2);
    }

    #[test]
    fn functional_reduce() {
        let tiles = vec![
            TileData::new("s1", 10.0),
            TileData::new("s2", 20.0),
            TileData::new("s3", 30.0),
        ];
        let sum = reduce(&tiles, 0.0, |acc, t| acc + t.value);
        assert!((sum - 60.0).abs() < 1e-9);
    }

    #[test]
    fn transform_result_helpers() {
        let pass = TransformResult::pass();
        assert!(pass.is_pass());
        assert!(!pass.is_drop());

        let drop = TransformResult::Drop;
        assert!(drop.is_drop());

        let err = TransformResult::Error("bad".to_string());
        assert!(!err.is_pass());
    }

    #[test]
    fn serialization_roundtrip_tile_data() {
        let t = TileData::new("s1", 42.0).with_tag("env", "prod");
        let json = serde_json::to_string(&t).unwrap();
        let back: TileData = serde_json::from_str(&json).unwrap();
        assert_eq!(back.id, t.id);
        assert_eq!(back.sensor_id, t.sensor_id);
        assert_eq!(back.tags, t.tags);
    }

    #[test]
    fn pipeline_len_and_empty() {
        let empty = TransformPipeline::new();
        assert!(empty.is_empty());
        assert_eq!(empty.len(), 0);

        let p = TransformPipeline::new().chain(ScaleTransform::scale(2.0));
        assert!(!p.is_empty());
        assert_eq!(p.len(), 1);
    }
}
