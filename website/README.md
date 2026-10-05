# Web de openrailsrs

[Sitio publicado](https://cavazquez.github.io/openrailsrs/). Presenta la partida,
el recorrido de referencia y cómo probarla. Las páginas técnicas enlazan la
documentación canónica y muestran el alcance real de compatibilidad.

## Estructura

- `src/`: contenido de seis páginas: inicio, experiencia, empezar, estado, física y comparación OR.
- `templates/layout.html`: navegación, pie, accesibilidad y metadatos compartidos.
- `site.json`: configuración, navegación y descripciones SEO.
- `css/style.css`: diseño responsive y foco de teclado.
- `js/site.js`: menú móvil, estaciones, galería y copiar comandos.
- `assets/`: marca SVG y capturas reales WebP, con procedencia y hashes.
- Los seis HTML de la raíz son **generados**; no editarlos a mano.

No necesita framework, Node, CDN ni fuentes externas. Páginas, enlaces y comandos
siguen accesibles sin JavaScript. Bevy, Rust mínimo y la referencia OR se leen
del repositorio para evitar versiones contradictorias.

## Editar, comprobar y publicar

```bash
python3 scripts/build_website.py
./scripts/sync_website_to_docs.sh
python3 -m unittest discover -s scripts -p test_website.py
python3 -m http.server 8890 --bind 127.0.0.1 --directory website
```

Revisar escritorio y móvil, menú con Escape, estaciones, galería, copia de
comandos, tildes, foco y ausencia de scroll horizontal. `build_website.py --check`
rechaza HTML desactualizado. Los tests verifican anclas, enlaces locales,
recursos, textos alternativos y sincronización con `docs/`. El sync conserva la
documentación y fixtures existentes.

El workflow `.github/workflows/pages.yml` reconstruye, valida y publica `docs/`
en GitHub Pages al cambiar la web, documentación o sus herramientas en `main`.
Confirmar el workflow antes de anunciar la publicación. La copia generada
versionada permite conservar también instalaciones Pages que publican `/docs`
desde la rama.
