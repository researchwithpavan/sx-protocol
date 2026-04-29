import os
import pytest

from sx import SxMessage, sx_library_available


@pytest.mark.skipif(not (os.environ.get("SX_FFI_LIB") or sx_library_available()), reason="requires built sx-ffi shared library")
def test_parse_encode_decode_hash_roundtrip():
    msg = SxMessage.parse_text('{name:"Asha",active:true}')
    encoded = msg.to_binary()
    decoded = SxMessage.from_binary(encoded)
    text = msg.to_text()
    decoded_text = decoded.to_text()
    assert "Asha" in text
    assert "Asha" in decoded_text
    h = msg.logical_hash()
    h2 = decoded.logical_hash()
    assert len(h) == 32
    assert h == h2
    decoded.close()
    msg.close()
