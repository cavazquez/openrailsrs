# Chiltern (placeholder import)

Resultado de `openrailsrs import-msts` sobre la ruta MSTS Chiltern (Open Rails 1.6.1, Wine).

| Campo | Valor |
|-------|--------|
| Ruta MSTS | `~/Documentos/Open Rails/Content/Chiltern/ROUTES/Chiltern` |
| Estado | `track.toml` vacío (0 nodos / 0 edges) — el importador aún no extrae topología de esta ruta |

Sirve como marcador para futuras comparaciones `compare-or` cuando el import produzca grafo válido.

```bash
openrailsrs import-msts "/path/to/Chiltern" --out-dir examples/chiltern
```

Baselines Open Rails de la misma ruta: `examples/baselines/chiltern_birmingham/`.
