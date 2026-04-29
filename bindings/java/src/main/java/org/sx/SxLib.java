package org.sx;

import com.sun.jna.*;
import com.sun.jna.ptr.PointerByReference;

public interface SxLib extends Library {
    SxLib INSTANCE = Native.load(System.getenv().getOrDefault("SX_FFI_LIB", "sx_ffi"), SxLib.class);

    int sx_message_parse_text(String input, PointerByReference outMsg, PointerByReference outErr);
    int sx_message_to_text(Pointer msg, PointerByReference outText, PointerByReference outErr);
    void sx_message_free(Pointer msg);
    void sx_string_free(Pointer text);
    String sx_version();
}
