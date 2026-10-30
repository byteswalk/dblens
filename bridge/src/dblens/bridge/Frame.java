package dblens.bridge;

import java.io.*;
import java.nio.charset.StandardCharsets;

/** 线上格式，Rust 端见 crates/dblens-driver-jdbc/src/frame.rs，两边必须同时改。 */
final class Frame {
    static final int MAX = 256 * 1024 * 1024;
    final long id;
    final int kind;
    final DataInputStream in;

    private Frame(long id, int kind, byte[] body) {
        this.id = id;
        this.kind = kind;
        this.in = new DataInputStream(new ByteArrayInputStream(body));
    }

    /** 读一帧；对端关闭时返回 null。 */
    static Frame read(DataInputStream s) throws IOException {
        int len;
        try {
            len = s.readInt();
        } catch (EOFException e) {
            return null;
        }
        if (len < 9 || len > MAX) throw new IOException("bad frame length " + len);
        long id = s.readLong();
        int kind = s.readUnsignedByte();
        byte[] body = new byte[len - 9];
        s.readFully(body);
        return new Frame(id, kind, body);
    }

    static String str(DataInputStream in) throws IOException {
        byte[] b = new byte[in.readInt()];
        in.readFully(b);
        return new String(b, StandardCharsets.UTF_8);
    }

    /** 响应构造器。 */
    static final class Out {
        final ByteArrayOutputStream buf = new ByteArrayOutputStream();
        final DataOutputStream d = new DataOutputStream(buf);

        void str(String s) throws IOException {
            byte[] b = s.getBytes(StandardCharsets.UTF_8);
            d.writeInt(b.length);
            d.write(b);
        }

        /** 整帧写出；stdout 由多个线程共用，必须持锁。 */
        void send(DataOutputStream out, long id, int kind) throws IOException {
            byte[] b = buf.toByteArray();
            synchronized (out) {
                out.writeInt(b.length + 9);
                out.writeLong(id);
                out.writeByte(kind);
                out.write(b);
                out.flush();
            }
        }
    }
}
