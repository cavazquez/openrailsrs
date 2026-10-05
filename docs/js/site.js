/* Progressive enhancement: content, navigation and commands work without JS. */
document.body.classList.add('js');
const contentFilters=document.querySelector('[data-content-filters]');
if(contentFilters){
  contentFilters.hidden=false;
  const search=document.querySelector('#content-search');
  const kind=document.querySelector('#content-kind');
  const cards=Array.from(document.querySelectorAll('[data-content-card]'));
  const normalize=text=>text.normalize('NFD').replace(/[\u0300-\u036f]/g,'').toLowerCase();
  const filter=()=>{
    let count=0;
    for(const card of cards){
      const match=normalize(card.textContent).includes(normalize(search.value))&&(kind.value==='all'||card.dataset[kind.value]==='true');
      card.hidden=!match;if(match)count++;
    }
    document.querySelector('#content-count').textContent=`${count} de ${cards.length} rutas`;
  };
  search.addEventListener('input',filter);kind.addEventListener('change',filter);filter();
}
const menuButton = document.querySelector('.menu-toggle');
const navigation = document.querySelector('#main-nav');
function closeMenu() {
  navigation.classList.remove('is-open');
  menuButton.setAttribute('aria-expanded', 'false');
}
menuButton.addEventListener('click', () => {
  const open = menuButton.getAttribute('aria-expanded') !== 'true';
  navigation.classList.toggle('is-open', open);
  menuButton.setAttribute('aria-expanded', String(open));
});
navigation.addEventListener('click', e => { if (e.target.closest('a')) closeMenu(); });
document.addEventListener('keydown', e => {
  if (e.key === 'Escape' && menuButton.getAttribute('aria-expanded') === 'true') {
    closeMenu();
    menuButton.focus();
  }
});

const stations = [
  ['Northolt Park', 'Acá empieza la partida. El tren está detenido; el monitor te avisa cuándo podés cerrar las puertas y salir.'],
  ['South Ruislip', 'Es la primera parada después de salir de Northolt Park. El monitor marca la distancia al punto donde tenés que frenar.'],
  ['West Ruislip', 'Acá termina el servicio corto. Si elegiste el de seis estaciones, la próxima parada es Denham.'],
  ['Denham', 'La cuarta estación del recorrido. Después del embarque seguís hacia Denham Golf Course.'],
  ['Denham Golf Course', 'Te queda una estación. Cerrá las puertas cuando se autorice la salida y continuá hacia Gerrards Cross.'],
  ['Gerrards Cross', 'El destino del servicio extendido. Al terminar la parada aparece el resumen con las llegadas y demoras.'],
];
document.querySelectorAll('[data-station]').forEach(button => {
  button.addEventListener('click', () => {
    const index = Number(button.dataset.station);
    if (!stations[index]) return;
    document.querySelectorAll('[data-station]').forEach(b => b.setAttribute('aria-pressed', String(b === button)));
    document.querySelector('#station-index').textContent = `Estación ${index + 1} / 6`;
    document.querySelector('#station-name').textContent = stations[index][0];
    document.querySelector('#station-description').textContent = stations[index][1];
  });
});

const views = {
  cabina: ['Cabina 3D original del Pullman y HUD de conducción', 'Pullman: cabina 3D e información de conducción.'],
  exterior: ['Formación Blue Pullman en Northolt Park vista desde el exterior', 'El Pullman en Northolt Park, visto desde afuera.'],
  noche: ['Viaje nocturno en Chiltern con faros sobre la vía y estrellas', 'Chiltern de noche, con cielo despejado y faros encendidos.'],
  nieve: ['Formación Pullman junto a un andén y terreno cubiertos de nieve', 'El tren y el andén durante una nevada.'],
};
document.querySelectorAll('[data-view]').forEach(button => {
  button.addEventListener('click', () => {
    const key = button.dataset.view;
    if (!views[key]) return;
    const image = document.querySelector('#gallery-image');
    image.src = `assets/${key}.webp`;
    image.alt = views[key][0];
    document.querySelector('#gallery-caption').textContent = views[key][1];
    document.querySelectorAll('[data-view]').forEach(b => b.setAttribute('aria-pressed', String(b === button)));
  });
});

document.querySelectorAll('.copy-button').forEach(button => {
  button.addEventListener('click', async () => {
    const code = button.closest('.code-box').querySelector('code');
    let deadline;
    button.textContent = 'Copiando…';
    try {
      await Promise.race([
        navigator.clipboard.writeText(code.textContent),
        new Promise((_, reject) => { deadline = setTimeout(() => reject(new Error('Clipboard unavailable')), 500); }),
      ]);
      button.textContent = 'Copiado';
    } catch {
      const selection = window.getSelection();
      const range = document.createRange();
      range.selectNodeContents(code);
      selection.removeAllRanges();
      selection.addRange(range);
      button.textContent = 'Seleccionado';
    } finally {
      clearTimeout(deadline);
    }
  });
});
