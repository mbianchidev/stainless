const done = arguments[arguments.length - 1];
(async () => {
  const settings = await import('chrome://settings/settings.js');
  const lazy = await import('chrome://settings/lazy_load.js');
  const {webUIListenerCallback} = await import('chrome://resources/js/cr.js');
  const router = settings.Router.getInstance();
  function roots(root = document) {
    return [root, ...Array.from(root.querySelectorAll('*'))
        .filter(element => element.shadowRoot)
        .flatMap(element => roots(element.shadowRoot))];
  }
  const find = selector => roots().map(root => root.querySelector(selector)).find(Boolean);
  const visible = element => element && element.getBoundingClientRect().height > 0;
  const check = (condition, message) => {
    if (!condition) {
      throw new Error(message);
    }
  };
  async function until(predicate, description) {
    const deadline = Date.now() + 15000;
    while (!predicate()) {
      if (Date.now() >= deadline) {
        throw new Error(`Timed out: ${description}`);
      }
      await new Promise(resolve => setTimeout(resolve, 50));
    }
  }
  await until(() => find('settings-main'), 'settings main');
  const main = find('settings-main');
  async function search(query) {
    const finished = new Promise(resolve =>
        main.addEventListener('search-finished', event => resolve(event.detail), {once: true}));
    await main.searchContents(query);
    return finished;
  }
  for (const key of [
    'syncAndNonPersonalizedServices', 'googleServicesPageTitle',
    'safeBrowsingSectionLabel', 'passwordsLeakDetectionLabel',
    'preloadingPageTitle', 'accessibleImageLabelsTitle', 'paymentsTitle',
  ]) {
    const result = await search(settings.loadTimeData.getString(key));
    check(result.matchCount === 0, `Unavailable service still appears in search: ${key}`);
  }
  for (const selector of [
    'settings-sync-page', 'settings-sync-controls-page', 'settings-account-page',
    'settings-google-services-page', 'settings-translate-page', 'settings-payments-page',
    'settings-live-caption', 'settings-ax-annotations-section',
    '#safeBrowsingRadioGroup', '#passwordsLeakToggle', '#preloadingToggle',
    '#googleWalletButton', '#chromeSigninUserChoiceSetting',
    '[pref-key="settings.a11y.enable_accessibility_image_labels"]',
  ]) {
    check(!find(selector), `Removed control remains in the DOM: ${selector}`);
  }
  check((await search(settings.loadTimeData.getString('localPasswordManager'))).matchCount > 0,
      'Local password settings disappeared from search');
  check((await search(settings.loadTimeData.getString('importTitle'))).matchCount > 0,
      'Browsing-data import disappeared from search');
  await search('');
  router.navigateTo(settings.routes.SECURITY);
  await until(() => visible(find('#manageCertificatesLinkRow')) &&
      visible(find('settings-secure-dns')),
      'retained certificates and custom secure DNS settings');
  check(find('#httpsFirstModeToggle') || find('#httpsOnlyModeToggle'),
      'HTTPS controls disappeared');
  router.navigateTo(settings.routes.SITE_SETTINGS_JAVASCRIPT_OPTIMIZER);
  await until(() => visible(find('#enableForAllSites')) &&
      visible(find('#blockForAllSites')), 'local JavaScript optimizer choices');
  check(!find('#blockForUnfamiliarSites'),
      'Safe Browsing-dependent JavaScript optimizer choice remains');
  router.navigateTo(settings.routes.PEOPLE);
  await until(() => visible(find('#profile-row')), 'local profile editor');
  find('#profile-row').click();
  await until(() => router.getCurrentRoute() === settings.routes.MANAGE_PROFILE,
      'account-free profile edit route');
  await until(() => {
    const input = find('settings-manage-profile')?.shadowRoot?.querySelector('#nameInput');
    return visible(input) && !input.disabled;
  }, 'editable local profile name');
  router.navigateTo(settings.routes.PEOPLE);

  // Only the import service boundary is synthetic. Never enumerate the host's browsers
  // or open its native file picker during this renderer/UI check.
  const original = lazy.ImportDataBrowserProxyImpl.getInstance();
  let transfers = 0;
  lazy.ImportDataBrowserProxyImpl.setInstance({
    initializeImportDialog: () => Promise.resolve([{
      index: 0, name: 'Synthetic bookmark file', profileName: '',
      history: false, favorites: true, passwords: false, search: false,
      autofillFormData: false,
    }]),
    importFromBookmarksFile() {
      transfers++;
      queueMicrotask(() => webUIListenerCallback(
          'import-data-status-changed', lazy.ImportDataStatus.SUCCEEDED));
    },
    importData() {
      throw new Error('Unexpected non-file source in the synthetic import check');
    },
  });
  try {
    await settings.PrefService.getInstance().whenInitialized();
    settings.PrefService.getInstance().setPrefValue('import_dialog_bookmarks', true);
    for (let iteration = 0; iteration < 2; iteration++) {
      await until(() => visible(find('#importDataDialogTrigger')), 'repeat import entry');
      find('#importDataDialogTrigger').click();
      await until(() => {
        const button = find('settings-import-data-dialog')?.shadowRoot?.querySelector('#import');
        return visible(button) && button.disabled === false;
      }, 'explicit import consent');
      const dialog = find('settings-import-data-dialog');
      dialog.shadowRoot.querySelector('#import').click();
      await until(() => {
        const button = dialog.shadowRoot.querySelector('#done');
        return visible(button) && !button.hidden;
      }, 'synthetic file-import response');
      dialog.shadowRoot.querySelector('#done').click();
      await until(() => router.getCurrentRoute() === settings.routes.PEOPLE,
          'return from import');
    }
    check(transfers === 2, 'Explicit import could not be repeated');
  } finally {
    lazy.ImportDataBrowserProxyImpl.setInstance(original);
  }
  return {
    removed_settings_search: true,
    retained_local_settings: true,
    repeatable_import_ui_with_synthetic_backend: true,
  };
})().then(done, error => done({error: String(error)}));
