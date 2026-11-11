const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');

const [typescriptPath, importerPath] = process.argv.slice(2);
assert(typescriptPath && importerPath,
    'Pass the pinned TypeScript module and password importer source');
const ts = require(path.resolve(typescriptPath));
const stores = [];
const replacements = [];
const resets = [];
const statuses = new Proxy({}, {get: (_target, name) => name});
let nextResult;
const success = () => ({
  status: statuses.kSuccess,
  displayedEntries: [],
  numberImported: 1,
  fileName: 'synthetic.csv',
});
const service = {
  async importPasswords(store) {
    stores.push(store);
    return nextResult;
  },
  async continueImport(ids) {
    replacements.push([...ids]);
    return success();
  },
  async resetImporter(deleteFile) {
    resets.push(deleteFile);
  },
};
class Element {
  constructor() {
    this.shadowRoot = {
      querySelector: selector =>
          selector === '#storePicker' ? {value: 'account'} : {checked: false},
    };
    this.$ = {selectFileButtonLinkRow: {focus() {}}, linkRow: {focus() {}}};
  }
}
const i18n = base => class extends base {
  i18n(key) {
    return key;
  }
  i18nAdvanced(key) {
    return key;
  }
};
function imports(id) {
  if (id.endsWith('/i18n_mixin.js')) {
    return {I18nMixin: i18n};
  }
  if (id.endsWith('/polymer_bundled.min.js')) {
    return {PolymerElement: Element};
  }
  if (id.endsWith('/assert.js')) {
    return {assert, assertNotReached: () => assert.fail('Unexpected import state')};
  }
  if (id.endsWith('/load_time_data.js')) {
    return {loadTimeData: {getString: key => key}};
  }
  if (id.endsWith('/plural_string_proxy.js')) {
    return {PluralStringProxyImpl: {
      getInstance: () => ({getPluralString: async (key, count) => `${key}: ${count}`}),
    }};
  }
  if (id === './password_manager.mojom-webui.js') {
    return {ImportEntryStatus: statuses, ImportResultsStatus: statuses};
  }
  if (id === './password_manager_proxy.js') {
    return {PasswordManagerImpl: {getInstance: () => service}};
  }
  if (id === './router.js') {
    return {Page: {PASSWORDS: 'passwords'}, Router: {getInstance: () => ({
      navigateTo() {},
    })}};
  }
  if (id === './user_utils_mixin.js') {
    return {UserUtilMixin: base => class extends base {
      executeIfTrustedVaultUnlocked(action) {
        action();
      }
    }};
  }
  if (id.endsWith('.html.js')) {
    return {getTemplate() {}};
  }
  if (id.endsWith('.css.js') || id.startsWith('chrome://resources/cr_elements/') ||
      id.startsWith('./dialogs/') || id === './site_favicon.js') {
    return {};
  }
  throw new Error(`Unexpected importer dependency: ${id}`);
}
const compiled = ts.transpileModule(fs.readFileSync(importerPath, 'utf8'), {
  compilerOptions: {module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022},
}).outputText;
const exportsObject = {};
const context = vm.createContext({
  customElements: {define() {}},
  chrome: {passwordsPrivate: {PasswordStoreSet: {DEVICE: 'device', ACCOUNT: 'account'}}},
  setTimeout: action => {queueMicrotask(action); return 0;},
});
vm.runInContext(
    `(function(require, exports) {${compiled}\n})`, context,
    {filename: importerPath})(imports, exportsObject);

(async () => {
  const importer = new exportsObject.PasswordsImporterElement();
  // Stale account state must not turn a file transfer into a cloud operation.
  importer.isAccountStoreUser = true;
  importer.isSyncingPasswords = true;
  importer.accountEmail = 'synthetic@example.invalid';
  nextResult = {...success(), displayedEntries: [{
    id: 101, status: statuses.kUnknownError, url: 'https://synthetic.invalid/',
    username: 'synthetic', password: 'synthetic-only',
  }]};
  importer.launchImport();
  await new Promise(setImmediate);
  assert.deepEqual(stores, ['device']);
  assert.equal(importer.showRowsWithUnknownErrorsSummary_, true);

  importer.conflictsSelectedForReplace_ = [101];
  nextResult = {...success(), status: statuses.kConflicts, displayedEntries: [{
    id: 202, status: statuses.kConflictProfile, url: 'https://synthetic.invalid/',
    username: 'synthetic', password: 'synthetic-refresh',
  }]};
  importer.launchImport();
  await new Promise(setImmediate);
  assert.deepEqual(stores, ['device', 'device']);
  assert.equal(importer.showRowsWithUnknownErrorsSummary_, false);
  assert.equal(importer.conflictsSelectedForReplace_.length, 0);
  assert.equal(replacements.length, 0, 'Conflicts must wait for user consent');
  importer.conflictsSelectedForReplace_ = [202];
  await importer.onReplaceClick_();
  assert.deepEqual(replacements, [[202]]);
  await importer.onCloseClick_();
  assert.deepEqual(resets, [false], 'The source file is not deleted without consent');

  importer.launchImport();
  await new Promise(setImmediate);
  assert.equal(importer.conflictsSelectedForReplace_.length, 0);
  await importer.onSkipClick_();
  assert.deepEqual(replacements, [[202], []]);
  assert.deepEqual(stores, ['device', 'device', 'device']);
  console.log('Production CSV import state: local-only repeat imports, fresh conflict consent and retained source files.');
})().catch(error => {
  console.error(error);
  process.exitCode = 1;
});
