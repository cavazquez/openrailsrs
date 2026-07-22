# Sitio web de openrailsrs

**Fuente de verdad:** esta carpeta (`website/`). Los HTML se publican en GitHub Pages vía `docs/` (sync automático).

URL: **https://cavazquez.github.io/openrailsrs/**

## Páginas

| Archivo | Contenido |
|---------|-----------|
| `index.html` | Landing: principios, características resumidas, crates, CLI |
| `fisica.html` | Referencia de conceptos físicos (TOC lateral, anclas por subsistema) |
| `paridad-or.html` | Comparación Open Rails vs openrailsrs, métricas, roadmap OR-P |
| `css/style.css` | Estilos compartidos (tema oscuro, tablas, layout docs) |

## Editar y publicar

1. Modificá archivos en `website/`.
2. Sincronizá a `docs/` (GitHub Pages legacy lee `/docs` en `main`):

```bash
./scripts/sync_website_to_docs.sh
git add website/ docs/
git commit -m "..."
git push
```

3. El workflow [`.github/workflows/pages.yml`](../.github/workflows/pages.yml) también corre `sync_website_to_docs.sh` antes del deploy por Actions.

**Settings → Pages:** conviene **Build and deployment → Source: GitHub Actions**. Si sigue en *Deploy from branch → /docs*, igual funciona mientras `docs/` tenga los HTML sincronizados.

## Ver localmente

```bash
python3 -m http.server 8080 --directory website
# http://localhost:8080/fisica.html
```

## Mantenimiento

- Actualizar métricas en `paridad-or.html` cuando cambien umbrales Chiltern/SCE en CI.
- Añadir secciones en `fisica.html` al implementar nuevas fases OR-P (p. ej. OR-P6c skid).
- Documentación canónica en el repo: `docs/OR_PARITY.md`, `docs/OR_PARITY.md`.
