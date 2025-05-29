import { createMergeableStore } from 'https://cdn.jsdelivr.net/npm/tinybase@5.4.4/+esm';
import { createWsSynchronizer } from 'https://cdn.jsdelivr.net/npm/tinybase@5.4.4/synchronizers/synchronizer-ws-client/+esm';
import { createLocalPersister } from 'https://cdn.jsdelivr.net/npm/tinybase@5.4.4/persisters/persister-browser/+esm';
import { v4 as uuidv4 } from 'https://cdn.jsdelivr.net/npm/uuid@11.1.0/+esm';

async function createLocalOnlyStore() {
    const store = createMergeableStore('bluegreen_local');
    console.log('Local Store Created:', store);

    const persister = createLocalPersister(store, 'bluegreen_local_v1');
    await persister.load();
    await persister.startAutoSave()
    console.log('Local Persister loaded and auto-save started');

    return store;
}

export async function createSharedStore(wsSyncUrl) {
    const store = createMergeableStore('bluegreen_shared');
    console.log('Shared Store Created:', store);

    const clientSynchronizer = await createWsSynchronizer(
        store,
        new WebSocket(wsSyncUrl),
    );
    console.log('Shared WebSocket synchronizer created:', clientSynchronizer);
    await clientSynchronizer.startSync();
    console.log('Shared WebSocket synchronizer started');

    const persister = createLocalPersister(store, 'bluegreen_shared_v1');
    await persister.load();
    await persister.startAutoSave()
    console.log('Shared Persister loaded and auto-save started');

    return store;
}

export async function createModel(mapCenter, wsSyncUrl) {
    const localStore = await createLocalOnlyStore();
    const sharedStore = await createSharedStore(wsSyncUrl);

    var clientId = localStore.getValue('clientId');
    if (!clientId) {
        clientId = uuidv4();
        localStore.setValue('clientId', clientId);
        console.log('New client ID set:', clientId);
    } else {
        console.log('Existing client ID:', clientId);
    }
    
    return new Model(clientId, mapCenter, sharedStore);
}

class Model {
    constructor(clientId, initialCenter, store) {
        this.clientId = clientId;
        this.store = store;
        this.store.setTable('centers', { 
            [clientId]: { 
                lat: initialCenter.lat,
                lng: initialCenter.lng 
            } 
        });
    }

    setCurrentView(center) {
        this.store.setRow('centers', this.clientId, {
            lat: center.lat,
            lng: center.lng
        });
        console.log(this.store.getTables());
        console.log('Current view set to:', center);
    }

    addCentersListener(listenerFn) {
        var ignore = null;
        this.store.addTableListener(
            'centers',
            (store, tableId, getCellChange) => {
                const collatedChanges = [];
                const table = store.getTable(tableId);
                for (const [clientId, row] of Object.entries(table)) {
                    const latCellChange = getCellChange(tableId, clientId, 'lat');
                    const lonCellChange = getCellChange(tableId, clientId, 'lon');
                    const changed = latCellChange[0] || lonCellChange[0];
                    if (changed) {
                        const center = {
                            id: clientId,
                            lat: row.lat,
                            lng: row.lng
                        };
                        collatedChanges.push(center);
                    }
                };
                
                console.log("collatedChanges", collatedChanges);
                listenerFn(collatedChanges);
            },
        );
        const initialPositions = [];
        for (const [clientId, row] of Object.entries(this.store.getTable('centers'))) {
            const center = {
                id: clientId,
                lat: row.lat,
                lng: row.lng
            };
            initialPositions.push(center);
        }
        console.log("Initial positions", initialPositions);
        listenerFn(initialPositions);
    }
}