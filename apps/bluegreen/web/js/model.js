import { createMergeableStore } from 'https://cdn.jsdelivr.net/npm/tinybase@5.4.4/+esm';
import { v4 as uuidv4 } from 'https://cdn.jsdelivr.net/npm/uuid@11.1.0/+esm'
import { createLocalPersister } from 'https://cdn.jsdelivr.net/npm/tinybase@5.4.4/persisters/persister-browser/+esm';


export async function createModel(center) {
    const store = createMergeableStore('bluegreen');
    console.log('Model created with store:', store);

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
    
    return new Model(clientId, center, store);
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
}