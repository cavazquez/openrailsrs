# Sitio web de openrailsrs

Página estática en esta carpeta. Se publica con **GitHub Actions** (no desde `/docs`).

## Publicación (GitHub Pages)

1. En el repo: **Settings → Pages → Build and deployment**
2. **Source:** `GitHub Actions` (no “Deploy from a branch”)
3. Cada push a `main` que toque `website/` dispara el workflow [`.github/workflows/pages.yml`](../.github/workflows/pages.yml)

URL: **https://cavazquez.github.io/openrailsrs/**

Deploy manual: **Actions → Deploy website → Run workflow**

## Ver localmente

```bash
python3 -m http.server 8080 --directory website
# http://localhost:8080
```

## Archivos

| Archivo | Rol |
|---------|-----|
| `index.html` | Landing: características, crates, CLI, ejemplos |
| `css/style.css` | Estilos (tema oscuro, responsive) |
| `.nojekyll` | Evita que GitHub Pages procese el sitio con Jekyll |
