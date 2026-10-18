(() => {
  const modes = ['light', 'dark', 'system'];
  const labels = { light: '浅色模式', dark: '深色模式', system: '跟随系统' };
  let mode = 'system';
  try { mode = localStorage.getItem('theme') || 'system'; } catch {}
  if (!modes.includes(mode)) mode = 'system';
  const apply = () => {
    if (mode === 'system') delete document.documentElement.dataset.theme;
    else document.documentElement.dataset.theme = mode;
  };
  apply();
  document.addEventListener('DOMContentLoaded', () => {
    const button = document.querySelector('[data-theme-toggle]');
    if (!button) return;
    const update = () => {
      const next = modes[(modes.indexOf(mode) + 1) % modes.length];
      const label = `当前${labels[mode]}，切换为${labels[next]}`;
      button.setAttribute('aria-label', label);
      button.setAttribute('title', label);
      button.dataset.themeMode = mode;
      button.querySelectorAll('[data-theme-icon]').forEach(icon => {
        icon.toggleAttribute('hidden', icon.dataset.themeIcon !== mode);
      });
    };
    update();
    button.addEventListener('click', () => {
      mode = modes[(modes.indexOf(mode) + 1) % modes.length];
      apply();
      update();
      try { localStorage.setItem('theme', mode); } catch {}
    });
  });
})();
