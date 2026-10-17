(() => {
  let mode = 'system';
  try { mode = localStorage.getItem('theme') || 'system'; } catch {}
  if (!['system', 'light', 'dark'].includes(mode)) mode = 'system';
  const apply = () => {
    if (mode === 'system') delete document.documentElement.dataset.theme;
    else document.documentElement.dataset.theme = mode;
  };
  apply();
  document.addEventListener('DOMContentLoaded', () => {
    const buttons = document.querySelectorAll('[data-theme-mode]');
    const update = () => buttons.forEach(button => {
      button.setAttribute('aria-pressed', String(button.dataset.themeMode === mode));
    });
    update();
    buttons.forEach(button => button.addEventListener('click', () => {
      mode = button.dataset.themeMode;
      apply();
      update();
      try { localStorage.setItem('theme', mode); } catch {}
    }));
  });
})();
