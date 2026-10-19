import base64
import ctypes
import ctypes.util
import json
import sys

# Only GitHub can decrypt this sealed box; no private key or password is exported.
public_key = base64.b64decode('Oz5qHZE8a57VP9rc0oSl+c9hfZShkqEFYbpEi8cie38=')
value = json.load(sys.stdin)['value'].encode('utf-8')
sodium = ctypes.CDLL(ctypes.util.find_library('sodium'))
sodium.sodium_init.restype = ctypes.c_int
if sodium.sodium_init() < 0:
    raise RuntimeError('Sealed-box initialization failed')
sodium.crypto_box_seal.argtypes = [ctypes.c_void_p, ctypes.c_void_p, ctypes.c_ulonglong, ctypes.c_void_p]
sodium.crypto_box_seal.restype = ctypes.c_int
ciphertext = ctypes.create_string_buffer(len(value) + 48)
if sodium.crypto_box_seal(ciphertext, value, len(value), public_key) != 0:
    raise RuntimeError('Sealed-box encryption failed')
print(json.dumps({'key_id': '3380204578043523366', 'encrypted_value': base64.b64encode(ciphertext.raw).decode('ascii')}))
