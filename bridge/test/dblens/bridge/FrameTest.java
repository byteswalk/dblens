package dblens.bridge;

import java.io.*;

/** 与 Rust 端共用样例，防止两边线上格式走样。 */
public final class FrameTest {
    public static void main(String[] a) throws Exception {
        // 粘贴 frame.rs 测试 execute_request_matches_the_shared_sample 中的十六进制
        String hex = "0000002a000000000000000702000000000000000200000008"
            + "73656c656374203f00000001020000000000000001";
        byte[] b = new byte[hex.length() / 2];
        for (int i = 0; i < b.length; i++) b[i] = (byte) Integer.parseInt(hex.substring(i * 2, i * 2 + 2), 16);
        Frame f = Frame.read(new DataInputStream(new ByteArrayInputStream(b)));
        check(f.id == 7 && f.kind == 2, "head");
        check(f.in.readLong() == 2, "conn");
        check(Frame.str(f.in).equals("select ?"), "sql");
        check(f.in.readInt() == 1, "count");
        check(f.in.readUnsignedByte() == 2 && f.in.readLong() == 1, "param");
        Frame.Out o = new Frame.Out();
        o.d.writeBoolean(false); o.d.writeInt(1); o.d.writeByte(5); o.str("数洞");
        ByteArrayOutputStream sink = new ByteArrayOutputStream();
        o.send(new DataOutputStream(sink), 9, 132);
        check(sink.size() == 4 + 8 + 1 + 1 + 4 + 1 + 4 + 6, "out size");
        System.out.println("ok");
    }

    private static void check(boolean c, String m) {
        if (!c) throw new AssertionError(m);
    }
}
