# Sitio web de openrailsrs

Página estática que describe las características del proyecto. Sin dependencias de build.

## Ver localmente

```bash
# Desde la raíz del repo
python3 -m http.server 8080 --directory website

# Abrir en el navegador
# http://localhost:8080
```

También podés abrir `website/index.html` directamente en el navegador (los enlaces a docs del repo usan rutas relativas `../`).

## Contenido

- **index.html** — landing con principios, grid de características, tabla de crates, CLI, ejemplos y enlaces al roadmap.
- **css/style.css** — estilos (tema oscuro, responsive).

## Publicar

Cualquier hosting estático sirve la carpeta `website/` tal cual (GitHub Pages, Netlify, nginx, etc.).

Para GitHub Pages desde la raíz del repo, configurá la source en **Settings → Pages → `/website`**.
