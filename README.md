# plato-transform

> Data transformation pipeline for PLATO tiles — scale, threshold, normalize, filter, map, reduce

## What This Does

plato-transform provides a composable pipeline for transforming tile data as it flows through PLATO. Built-in transforms handle scaling, thresholding (clamp or drop), and normalization. A functional API provides map/filter/reduce. The pipeline chains transforms and stops on the first Drop or Error.

## The Key Idea

Raw sensor data rarely arrives in the shape you need. A temperature reading might be in Fahrenheit when you need Celsius (scale). It might have noise spikes you want to eliminate (threshold). And you probably want it normalized to [0,1] for model input (normalize). plato-transform chains these operations: each transform either passes the tile (possibly modified), drops it, or errors out. The first non-pass result stops the pipeline.

## Install

```bash
cargo add plato-transform
```

## Quick Start

```rust
use plato_transform::*;

let pipeline = TransformPipeline::new()
    .chain(ThresholdTransform::drop_outside(0.0, 100.0))
    .chain(ScaleTransform::new(0.01, 0.0))
    .chain(NormalizeTransform::new(0.0, 1.0));

let tiles = vec![
    TileData::new("temp", 50.0),
    TileData::new("temp", 150.0),  // dropped
    TileData::new("temp", 75.0),
];
let passing = pipeline.apply_batch(tiles);
assert_eq!(passing.len(), 2);
```

## API Reference

### Built-in Transforms

| Transform | Description |
|---|---|
| `ScaleTransform::new(factor, offset)` | value = value * factor + offset |
| `ThresholdTransform::clamp(min, max)` | Clamp values to range |
| `ThresholdTransform::drop_outside(min, max)` | Drop tiles outside range |
| `NormalizeTransform::new(min, max)` | Normalize to [0,1] by (value-min)/(max-min) |

### TileData

```rust
TileData::new("sensor-1", 42.0)
    .with_confidence(0.9)
    .with_layer(2)
    .with_tag("room", "kitchen")
    .with_timestamp(1700000000000);
```

### TransformPipeline

```rust
let pipeline = TransformPipeline::new()
    .chain(transform1)
    .chain(transform2);
pipeline.apply(&mut tile);      // TransformResult
pipeline.apply_batch(tiles);    // Vec<TileData> (drops removed)
```

### Functional Helpers

```rust
map(&mut tiles, |v| v * 2.0);                        // Transform values
let kept = filter(tiles, |t| t.value > 0.0);         // Keep matching
let sum = reduce(&tiles, 0.0, |acc, t| acc + t.value); // Aggregate
```

### Custom Transforms

Implement the `Transform` trait:

```rust
impl Transform for MyTransform {
    fn name(&self) -> &str { "my-transform" }
    fn apply(&self, tile: &mut TileData) -> TransformResult {
        tile.value = tile.value.sqrt();
        TransformResult::pass()
    }
    fn box_clone(&self) -> Box<dyn Transform> { Box::new(self.clone()) }
}
```

## Testing

18 tests: tile construction, scale, threshold clamp/drop, normalize (including zero-range error), pipeline chaining/batch, functional map/filter/reduce, serialization.

## License

Apache-2.0
