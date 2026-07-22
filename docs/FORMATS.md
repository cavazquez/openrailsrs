# Formatos MSTS / auditoría

## Shape binario `.s`

Parser en `openrailsrs-formats` (compressed + classic). Pipeline viewer: LOD, matrices, prim_states intercalados con trilists (orden OR). Plan residual / notas: crate `formats` + tests `parse_compressed_binary_shape_*`.

Normal maps PBR: sidecar opcional `MiShape.s.pbr.json` (#44) — ver [`VIEWER3D_TESTING.md`](VIEWER3D_TESTING.md).

## Audit vehículo (OR ↔ OpenBVE)

```bash
openrailsrs audit-vehicle path/to/engine.eng
```

Checklist de campos ENG/WAG frente a parsers de referencia; no sustituye física OR ([`OR_PARITY.md`](OR_PARITY.md)).

## OpenBVE (referencia)

Útil para campos de tren/CVF/SMS; **no** es autoridad de física MSTS/OR. Licencia/uso: [`THIRD_PARTY.md`](THIRD_PARTY.md).

## Terceros / parsers

| Fuente | Uso |
|--------|-----|
| Open Rails (GPL) | Comportamiento y formatos; no copiar C# verbatim |
| OpenBVE | Checklist campos |
| Track Viewer / TSRE5 | Validación visual TDB — [`TRACK_MSTS.md`](TRACK_MSTS.md) |
