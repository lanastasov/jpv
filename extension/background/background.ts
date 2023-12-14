import { StorageChange, Tab, getBrowser } from '../lib/compat.js';
import { DomainSettings, loadDomainSetting, toDomainSettings } from '../lib/lib.js';

let B = getBrowser();

B.onTabUpdated.addListener(async (_tabId, _change, tab) => {
    await updateTab(tab);
});

B.onTabActivated.addListener(async ({tabId}) => {
    let tab = await B.tabsGet(tabId);
    await updateTab(tab);
});

B.onInstalled.addListener(async () => {
    let tabs = await B.tabsQuery({ active: true });

    for (let tab of tabs) {
        await updateTab(tab);
    }
});

B.onStorageChanged.addListener(async (changes: {[key: string]: StorageChange}) => {
    for (let key of Object.keys(changes)) {
        if (!key.startsWith('domain/')) {
            continue;
        }

        let { newValue } = changes[key];
        let setting = toDomainSettings(newValue);

        let [_, host] = key.split('/', 2);
        let tabs = await B.tabsQuery({ active: true });

        for (let tab of tabs) {
            if (!tab.url) {
                continue;
            }

            let url = new URL(tab.url);

            if (url.host !== host) {
                continue;
            }

            updateIcon(tab, setting);
        }
    }
});

async function updateTab(tab: Tab) {
    if (tab.url === undefined) {
        return;
    }

    let url = new URL(tab.url);
    let setting = await loadDomainSetting(url.host);
    await updateIcon(tab, setting);
}

async function updateIcon(tab: Tab, setting: DomainSettings) {
    if (setting.enabled) {
        await B.setIcon({ tabId: tab.id, path: { "256": '/icons/jpv-256.png' } });
    } else {
        await B.setIcon({ tabId: tab.id, path: { "256": '/icons/jpv-disabled-256.png' } });
    }
}
