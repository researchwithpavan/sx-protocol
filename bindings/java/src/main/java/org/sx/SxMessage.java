package org.sx;

import com.sun.jna.Pointer;
import com.sun.jna.ptr.PointerByReference;

public class SxMessage implements AutoCloseable {
    private Pointer handle;

    private SxMessage(Pointer handle) {
        this.handle = handle;
    }

    public static SxMessage parseText(String text) {
        PointerByReference outMsg = new PointerByReference();
        PointerByReference outErr = new PointerByReference();
        int status = SxLib.INSTANCE.sx_message_parse_text(text, outMsg, outErr);
        if (status != 0) {
            throw new RuntimeException("SX parse failed");
        }
        return new SxMessage(outMsg.getValue());
    }

    public String toText() {
        PointerByReference outText = new PointerByReference();
        PointerByReference outErr = new PointerByReference();
        int status = SxLib.INSTANCE.sx_message_to_text(handle, outText, outErr);
        if (status != 0) {
            throw new RuntimeException("SX to_text failed");
        }
        Pointer p = outText.getValue();
        String text = p.getString(0);
        SxLib.INSTANCE.sx_string_free(p);
        return text;
    }

    @Override
    public void close() {
        if (handle != null) {
            SxLib.INSTANCE.sx_message_free(handle);
            handle = null;
        }
    }
}
