# Referencias de código externo

Repositorios de simuladores ferroviarios usados como **referencia de lectura** durante el desarrollo de openrailsrs. Ninguno se redistribuye dentro del crate; viven como carpetas hermanas en el workspace `ProyectoOpenRails`.

| Proyecto | Ruta workspace | Licencia | Uso en openrailsrs |
|----------|----------------|----------|-------------------|
| [Open Rails](https://github.com/openrails/openrails) | `../openrails/` | GPL-3 | Rutas, física, viewer 3D, parsers MSTS — **referencia autoritativa** |
| [OpenBVE](https://github.com/leezer3/OpenBVE) | `../OpenBVE/` | BSD-2 (código nuevo) | CVF, SMS, parsers ENG/WAG — **referencia complementaria** |

## Reglas de uso

- **Open Rails (GPL):** estudiar comportamiento y formatos; no copiar código C# verbatim en Rust. Documentar hallazgos en `docs/OPEN_RAILS_VIEWER_3D.md`, `docs/OR_PARITY_ROADMAP.md`, etc.
- **OpenBVE (BSD-2):** portar ideas y algoritmos con atribución en comentarios del módulo Rust. Documentar en [`OPENBVE_REFERENCE.md`](OPENBVE_REFERENCE.md).

## Documentación por proyecto

- Open Rails: [`OPEN_RAILS_VIEWER_3D.md`](OPEN_RAILS_VIEWER_3D.md), [`OR_TRACE_COMPARISON.md`](OR_TRACE_COMPARISON.md), [`OR_PARITY_ROADMAP.md`](OR_PARITY_ROADMAP.md)
- OpenBVE: [`OPENBVE_REFERENCE.md`](OPENBVE_REFERENCE.md)
- Auditoría ENG/WAG: [`PARSER_CROSS_VALIDATION.md`](PARSER_CROSS_VALIDATION.md)
