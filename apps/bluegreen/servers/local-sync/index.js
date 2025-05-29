
import {createWsServer} from 'tinybase/synchronizers/synchronizer-ws-server';
import {WebSocketServer} from 'ws';

console.log("Hola");

const port = 8048;
const server = createWsServer(new WebSocketServer({port}));
console.log(`WebSocket server started on port ${port}`);