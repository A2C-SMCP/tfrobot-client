"""Loopback Socket.IO/REST fixture. stdout is the controller's event stream."""
import asyncio
import json
import logging
import platform
import importlib.metadata
import re
import time
import socketio
import uvicorn
from fastapi import FastAPI, Request
from fastapi.middleware.cors import CORSMiddleware

stage = 'setup'
connections = {}
transports = {}
history = []
sequence = 0

def record(kind, **data):
    print(json.dumps(dict(kind=kind, ts=time.time_ns() // 1_000_000,
                         mono=time.monotonic_ns() // 1_000_000, stage=stage, **data)), flush=True)

class HeartbeatLog(logging.Handler):
    def emit(self, entry):
        match = re.match(r'(\S+): (Sending packet PING|Received packet PONG|Client is gone)', entry.getMessage())
        if match:
            record({'Sending packet PING': 'ping', 'Received packet PONG': 'pong',
                    'Client is gone': 'ping_timeout'}[match[2]], sid=match[1])

logger = logging.getLogger('acceptance-engine')
logger.handlers = [HeartbeatLog()]
logger.setLevel(logging.INFO)
logger.propagate = False
sio = socketio.AsyncServer(async_mode='asgi', cors_allowed_origins='*',
                          ping_interval=25, ping_timeout=60, engineio_logger=logger)
app = FastAPI()
app.add_middleware(CORSMiddleware, allow_origins=['*'], allow_methods=['*'], allow_headers=['*'])

def message(content):
    global sequence
    sequence += 1
    item = dict(msgId=f'm{sequence}', content=content, additionalKwargs={}, attachments=None,
                createTimestamp=time.time_ns() // 1_000_000, creator=dict(uid='fixture', name='Fixture'),
                conversationId=42, role='user', msgType='text')
    history.append(item)
    return item

message('INITIAL-HISTORY')

@sio.on('connect', namespace='/chat')
async def connect(sid, environ, auth):
    if (auth or {}).get('token') != 'fixture':
        return False
    eio = sio.manager.eio_sid_from_sid(sid, '/chat')
    connections[sid] = eio
    transports[eio] = environ['asgi.scope']['acceptance_send']
    record('connect', sid=sid)

@sio.on('disconnect', namespace='/chat')
async def disconnect(sid, reason=None):
    connections.pop(sid, None)
    record('disconnect', sid=sid, reason=reason)

@sio.on('join_conversation', namespace='/chat')
async def join(sid, data):
    record('join', sid=sid, data=data)
    return None  # Current-server's empty ACK: exercises REST recovery policy.

@app.get('/api/v1/chat/conversations')
async def conversations():
    record('list_conversations')
    return dict(code=200, message='Success', data=dict(conversations=[dict(conversationId=42, title='Background fixture')], cursor=None))

@app.get('/api/v1/chat/conversations/42/messages')
async def messages():
    record('history', ids=[m['msgId'] for m in history])
    return dict(code=200, message='Success', data=dict(messages=history, events=[], cursor=None))

@app.get('/api/v1/chat/conversations/42/status')
async def status():
    return dict(code=200, message='Success', data=dict(working=False, task_id=None))

@app.post('/stage/{name}')
async def set_stage(name: str):
    global stage
    stage = name
    record('stage')
    return {}

@app.post('/snapshot')
async def snapshot(request: Request):
    record('snapshot', **await request.json())
    return {}

@app.post('/message/{name}')
async def send_message(name: str):
    item = message(name)
    record('message_sent', id=item['msgId'], content=name)
    await sio.emit('chat_message', item, namespace='/chat')
    return item

@app.post('/fault')
async def fault():
    # Withhold a persisted message from the socket, then close the actual transport.
    # Its appearance in the UI must therefore be caused by REST history recovery.
    item = message('MISSED-WHILE-OFFLINE')
    record('fault', id=item['msgId'])
    for eio in list(connections.values()):
        await transports[eio]({'type': 'websocket.close', 'code': 1012})
    return {}

@app.on_event('startup')
async def startup():
    # call_soon runs after startup completes; no readiness polling required.
    asyncio.get_running_loop().call_soon(lambda: record('server_ready', system=platform.platform(),
        versions={name: importlib.metadata.version(name) for name in ['python-socketio', 'python-engineio', 'uvicorn', 'fastapi']}))

socket_app = socketio.ASGIApp(sio, app)
async def transport_app(scope, receive, send):
    if scope['type'] == 'websocket':
        scope['acceptance_send'] = send
    await socket_app(scope, receive, send)

if __name__ == '__main__':
    uvicorn.run(transport_app, host='127.0.0.1', port=18766,
                log_level='warning', ws_ping_interval=None)
