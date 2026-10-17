/* Keep the optional header control on mdBook's own theme and storage path. */
(() => {
    const control = document.querySelector('.site-appearance');
    const light = document.getElementById('mdbook-theme-light');
    const dark = document.getElementById('mdbook-theme-navy');
    if (!control || !light || !dark) return;

    const isDark = () => ['navy', 'coal', 'ayu'].some(
        theme => document.documentElement.classList.contains(theme)
    );
    const update = () => {
        const next = isDark() ? 'Light' : 'Dark';
        control.querySelector('.appearance-label').textContent = next;
        control.setAttribute('aria-label', `Switch to ${next.toLowerCase()} appearance`);
    };
    control.addEventListener('click', () => (isDark() ? light : dark).click());
    new MutationObserver(update).observe(document.documentElement, {
        attributes: true, attributeFilter: ['class']
    });
    update();
    control.hidden = false;

    const page = location.pathname.split('/').pop() || 'index.html';
    document.querySelectorAll('.site-nav a').forEach(link => {
        if (new URL(link.href).pathname.split('/').pop() === page && !link.hash) {
            link.setAttribute('aria-current', 'page');
        }
    });
})();
