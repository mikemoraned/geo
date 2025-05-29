import { createMergeableStore } from 'https://cdn.jsdelivr.net/npm/tinybase@5.4.4/+esm';
import { createWsSynchronizer } from 'https://cdn.jsdelivr.net/npm/tinybase@5.4.4/synchronizers/synchronizer-ws-client/+esm';
import { createLocalPersister } from 'https://cdn.jsdelivr.net/npm/tinybase@5.4.4/persisters/persister-browser/+esm';
import { v4 as uuidv4 } from 'https://cdn.jsdelivr.net/npm/uuid@11.1.0/+esm';

export async function createModel(mapCenter, wsSyncUrl) {
    const store = createMergeableStore('bluegreen');
    console.log('Model created with store:', store);

    const clientSynchronizer = await createWsSynchronizer(
        store,
        new WebSocket(wsSyncUrl),
    );
    console.log('WebSocket synchronizer created:', clientSynchronizer);
    await clientSynchronizer.startSync();
    console.log('WebSocket synchronizer started');

    const persister = createLocalPersister(store, 'bluegreen_v2');
    await persister.load();
    await persister.startAutoSave()
    console.log('Persister loaded and auto-save started');

    var clientId = store.getValue('clientId');
    if (!clientId) {
        clientId = uuidv4();
        store.setValue('clientId', clientId);
        console.log('New client ID set:', clientId);
    } else {
        console.log('Existing client ID:', clientId);
    }
    
    return new Model(clientId, mapCenter, store);
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
                const collated = [];
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
                        collated.push(center);
                    }
                };
                
                listenerFn(collated);
            },
        );
    }
}