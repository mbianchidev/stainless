const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');

const [typescriptPath, settingsPath] = process.argv.slice(2);
assert(typescriptPath && settingsPath,
    'Pass the pinned Chromium TypeScript module and settings source directory');
const ts = require(path.resolve(typescriptPath));
const root = path.resolve(settingsPath);
const location = {origin: 'chrome://settings', pathname: '/', search: ''};
const history = {
  state: null,
  replaceState(state, _title, url) {
    this.state = state;
    location.pathname = url;
  },
  pushState(state, _title, url) {
    this.state = state;
    location.pathname = url;
  },
};
const loadTimeData = {
  getBoolean: name => !['isGuest', 'searchSettingsUpdate'].includes(name),
  getString: name => name,
  getStringF: (name, ...values) => [name, ...values].join(' '),
};
const context = vm.createContext({
  URLSearchParams,
  window: {location, history, addEventListener() {}},
  document: {title: ''},
  chrome: {metricsPrivate: {recordSparseValueWithPersistentHash() {}}},
});
const modules = new Map();
function load(name) {
  if (modules.has(name)) {
    return modules.get(name);
  }
  const exports = {};
  modules.set(name, exports);
  const compiled = ts.transpileModule(
      fs.readFileSync(path.join(root, name + '.ts'), 'utf8'), {
        compilerOptions: {
          module: ts.ModuleKind.CommonJS,
          target: ts.ScriptTarget.ES2022,
        },
      }).outputText;
  const imports = id => {
    if (id === './i18n_setup.js') {
      return {loadTimeData};
    }
    if (id === './page_visibility.js') {
      return {pageVisibility: undefined};
    }
    if (id === './router.js') {
      return load('router');
    }
    if (id === 'chrome://resources/js/assert.js') {
      return {assert, assertNotReached: () => assert.fail('Unexpected route')};
    }
    if (id.includes('/lit/') || id.includes('/polymer/')) {
      return {dedupingMixin: mixin => mixin};
    }
    throw new Error(`Unexpected production routing dependency: ${id}`);
  };
  vm.runInContext(
      `(function(require, exports) {${compiled}\n})`, context,
      {filename: name + '.js'})(imports, exports);
  return exports;
}

const {buildRouter} = load('route');
for (const removed of [
  '/syncSetup', '/syncSetup/advanced', '/account', '/googleServices', '/signOut',
  '/ai', '/ai/gemini', '/ai/historySearch', '/ai/helpMeWrite',
  '/payments', '/shopping', '/enhancedAutofill',
  '/privacy/guide', '/content/location', '/content/ads',
]) {
  const router = buildRouter();
  assert.equal(router.getRouteForPath(removed), null, removed);
  location.pathname = removed;
  location.search = '?synthetic=unavailable';
  router.initializeRouteFromUrl();
  assert.equal(router.getCurrentRoute().path, '/', removed);
  assert.equal(location.pathname, '/', removed);
  assert.equal(router.getQueryParameters().toString(), '', removed);
}
for (const retained of [
  '/people', '/manageProfile', '/importData', '/autofill', '/contactInfo',
  '/identityDocs', '/travel',
  '/search', '/searchEngines', '/languages', '/accessibility', '/security',
  '/securityKeys', '/cookies', '/clearBrowserData', '/content/notifications',
  '/content/backgroundSync', '/content/siteData', '/content/camera',
  '/content/microphone', '/resetProfileSettings', '/help',
]) {
  const router = buildRouter();
  assert.equal(router.getRouteForPath(retained)?.path, retained, retained);
  location.pathname = retained;
  location.search = '';
  router.initializeRouteFromUrl();
  assert.equal(router.getCurrentRoute().path, retained, retained);
}
console.log('Production settings routing: removed-service deep links rejected; local/import routes retained.');
