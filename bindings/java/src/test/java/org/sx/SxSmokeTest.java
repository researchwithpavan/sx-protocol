package org.sx;

import org.junit.jupiter.api.Test;
import static org.junit.jupiter.api.Assertions.*;

public class SxSmokeTest {
    @Test
    void versionAvailable() {
        String version = SxLib.INSTANCE.sx_version();
        assertTrue(version.contains("SX Protocol"));
    }
}
