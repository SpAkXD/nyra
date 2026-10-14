// main.js: shared by every page
lucide.createIcons();

// Word-by-word title reveal: wrap each word in a clipping box.
document.querySelectorAll('.words').forEach((el) => {
  const delay = parseFloat(el.dataset.delay || 0);
  const step = parseFloat(el.dataset.stagger || 0.06);
  el.setAttribute('aria-label', el.textContent.trim());
  el.innerHTML = el.textContent.trim().split(' ').map((w, i) =>
    `<span class="w" aria-hidden="true"><span style="--d:${delay + i * step}s">${w} </span></span>`).join('');
});

// Stagger: each child of a .stagger list starts a little later.
document.querySelectorAll('.stagger').forEach((el) =>
  [...el.children].forEach((child, i) => { child.style.transitionDelay = `${0.05 + i * 0.08}s`; }));

// Reveal once, when 12% inside the viewport.
const io = new IntersectionObserver((entries) => {
  for (const e of entries) {
    if (!e.isIntersecting) continue;
    e.target.classList.add('in');
    e.target.dispatchEvent(new Event('reveal'));
    io.unobserve(e.target);
  }
}, { rootMargin: '0px 0px -12% 0px' });
document.querySelectorAll('.reveal, .stagger, .words:not([data-now])').forEach((el) => io.observe(el));
requestAnimationFrame(() => document.querySelectorAll('.words[data-now]').forEach((el) => el.classList.add('in')));

// Opening hours, Prague time. Index = weekday (0 = Sunday), minutes from midnight.
const HOURS = [null, [480, 960], [480, 960], [480, 960], [480, 960], [480, 960], [510, 690]];
function isOpenNow() {
  const parts = new Intl.DateTimeFormat('en-GB', {
    timeZone: 'Europe/Prague', weekday: 'short', hour: '2-digit', minute: '2-digit', hourCycle: 'h23',
  }).formatToParts(new Date());
  const get = (type) => parts.find((p) => p.type === type).value;
  const day = ['Sun', 'Mon', 'Tue', 'Wed', 'Thu', 'Fri', 'Sat'].indexOf(get('weekday'));
  const mins = Number(get('hour')) * 60 + Number(get('minute'));
  const slot = HOURS[day];
  return Boolean(slot && mins >= slot[0] && mins < slot[1]);
}
function updateBadges() {
  const open = isOpenNow();
  document.querySelectorAll('.badge').forEach((b) => {
    b.classList.toggle('open', open);
    b.querySelector('.label').textContent = open ? 'Nyní otevřeno' : 'Nyní zavřeno';
  });
}
updateBadges();
setInterval(updateBadges, 60000);
