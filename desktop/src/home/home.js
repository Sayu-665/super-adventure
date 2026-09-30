'use strict';

// Reads the display info the app puts in the fragment (#engine=DuckDuckGo&safe=off)
// and shows it as the caption. Text only; nothing here is privileged.
(function () {
  function render() {
    const params = new URLSearchParams(location.hash.slice(1));
    const engine = (params.get('engine') || '').replace(/[\u0000-\u001f]/g, '').slice(0, 40);
    if (!engine) return;
    const safeOn = params.get('safe') === 'on';
    const caption = document.getElementById('caption');
    caption.textContent = safeOn
      ? `Filtered search · SafeSearch on · ${engine}`
      : `Unrestricted search · SafeSearch off · ${engine}`;
    caption.classList.toggle('safe-on', safeOn);
  }

  render();
  window.addEventListener('hashchange', render);

  const input = document.querySelector('.search-input');
  document.querySelector('.search').addEventListener('submit', (e) => {
    if (!input.value.trim()) e.preventDefault();
  });
})();
