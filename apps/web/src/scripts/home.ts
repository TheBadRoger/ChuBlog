const hero = document.querySelector<HTMLElement>('.home-hero');
const navigation = document.querySelector<HTMLElement>('#navigation');

if (hero && navigation) {
  const reducedMotion = matchMedia('(prefers-reduced-motion: reduce)');
  let scrolling = false;
  let unlock: ReturnType<typeof setTimeout>;
  let touch: { x: number; y: number } | undefined;
  const inHero = () => hero.getBoundingClientRect().bottom > 1;
  const interactive = (target: EventTarget | null) => target instanceof Element &&
    !!target.closest('input, textarea, select, button, a, [contenteditable], [role="dialog"]');
  const finish = () => { scrolling = false; clearTimeout(unlock); };
  const advance = () => {
    if (scrolling || !inHero()) return;
    scrolling = true;
    navigation.scrollIntoView({ behavior: reducedMotion.matches ? 'instant' : 'smooth', block: 'start' });
    // Keep the remainder of a wheel/touch gesture from overshooting the navigation.
    unlock = setTimeout(finish, reducedMotion.matches ? 250 : 1100);
  };
  const cancel = () => {
    if (!scrolling) return;
    finish();
    window.scrollTo({ top: window.scrollY, behavior: 'instant' });
  };
  window.addEventListener('wheel', (event) => {
    if (event.ctrlKey || Math.abs(event.deltaX) >= Math.abs(event.deltaY)) return;
    if (event.deltaY < 0) { cancel(); return; }
    if (event.deltaY > 0 && (scrolling || inHero())) {
      event.preventDefault();
      advance();
    }
  }, { passive: false });
  window.addEventListener('touchstart', (event) => {
    const point = event.touches[0];
    touch = event.touches.length === 1 && !interactive(event.target) && inHero()
      ? { x: point.clientX, y: point.clientY } : undefined;
  }, { passive: true });
  window.addEventListener('touchmove', (event) => {
    if (!touch || event.touches.length !== 1) return;
    const point = event.touches[0];
    const dy = touch.y - point.clientY;
    if (dy < -24) { cancel(); touch = undefined; return; }
    if (Math.abs(point.clientX - touch.x) >= Math.abs(dy)) return;
    if (dy > 0 && (scrolling || inHero()) && event.cancelable) {
      event.preventDefault();
      if (dy > 24) advance();
    }
  }, { passive: false });
  window.addEventListener('touchend', () => { touch = undefined; }, { passive: true });
  window.addEventListener('touchcancel', () => { touch = undefined; }, { passive: true });
  window.addEventListener('keydown', (event) => {
    if (interactive(event.target) || event.ctrlKey || event.metaKey || event.altKey) return;
    if (['ArrowUp', 'PageUp', 'Home', 'Escape'].includes(event.key) || (event.key === ' ' && event.shiftKey)) { cancel(); return; }
    if (!event.shiftKey && ['ArrowDown', 'PageDown', ' '].includes(event.key) && (scrolling || inHero())) {
      event.preventDefault();
      advance();
    }
  });
  hero.querySelector<HTMLAnchorElement>('.hero-scroll')?.addEventListener('click', (event) => {
    event.preventDefault();
    advance();
    navigation.querySelector<HTMLAnchorElement>('a')?.focus({ preventScroll: true });
  });
  window.addEventListener('resize', cancel, { passive: true });
  window.addEventListener('pagehide', finish);
}
