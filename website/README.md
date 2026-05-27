# Sitio web de openrailsrs

Página estática en esta carpeta. Se publica con **GitHub Actions** (no desde `/docs`).

URL: **https://cavazquez.github.io/openrailsrs/**

## Páginas

| Archivo | Contenido |
|---------|-----------|
| `index.html` | Landing: principios, características resumidas, crates, CLI |
| `fisica.html` | Referencia de conceptos físicos (TOC lateral, anclas por subsistema) |
| `paridad-or.html` | Comparación Open Rails vs openrailsrs, métricas, roadmap OR-P |
| `css/style.css` | Estilos compartidos (tema oscuro, tablas, layout docs) |

## Publicación (GitHub Pages)

1. En el repo: **Settings → Pages → Build and deployment**
2. **Source:** `GitHub Actions`
3. Cada push a `main` que toque `website/` dispara [`.github/workflows/pages.yml`](../.github/workflows/pages.yml)

Deploy manual: **Actions → Deploy website → Run workflow**

## Ver localmente

```bash
python3 -m http.server 8080 --directory website
# http://localhost:8080
# http://localhost:8080/fisica.html
# http://localhost:8080/paridad-or.html
```

## Mantenimiento

- Actualizar métricas en `paridad-or.html` cuando cambien umbrales Chiltern/SCE en CI.
- Añadir secciones en `fisica.html` al implementar nuevas fases OR-P (p. ej. OR-P6c skid).
- La documentación canónica sigue en el repo: `docs/OR_PARITY_ROADMAP.md`, `CALIBRATION.md`.
