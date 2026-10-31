package dblens.bridge;

import java.io.*;
import java.math.BigDecimal;
import java.sql.*;
import java.time.OffsetDateTime;
import java.time.format.DateTimeFormatter;

/** JDBC 值与线上值的转换。值的标记见 Frame。 */
final class Values {
    /** Oracle 的 TIMESTAMP WITH TIME ZONE 类型码（oracle.jdbc.OracleTypes.TIMESTAMPTZ）。 */
    private static final int ORACLE_TIMESTAMPTZ = -101;
    private static final int ORACLE_BINARY_FLOAT = 100;
    private static final int ORACLE_BINARY_DOUBLE = 101;
    private static final DateTimeFormatter TS = DateTimeFormatter.ofPattern("yyyy-MM-dd'T'HH:mm:ss.SSSSSS");

    static void bind(PreparedStatement p, int i, DataInputStream in) throws Exception {
        switch (in.readUnsignedByte()) {
            case 0: p.setNull(i, Types.VARCHAR); break;
            case 1: p.setBoolean(i, in.readBoolean()); break;
            case 2: p.setLong(i, in.readLong()); break;
            case 3: p.setDouble(i, in.readDouble()); break;
            case 4: p.setBigDecimal(i, new BigDecimal(Frame.str(in))); break;
            case 5: p.setString(i, Frame.str(in)); break;
            case 6: { byte[] b = new byte[in.readInt()]; in.readFully(b); p.setBytes(i, b); break; }
            case 7: p.setDate(i, Date.valueOf(Frame.str(in))); break;
            case 8: p.setTime(i, Time.valueOf(Frame.str(in).substring(0, 8))); break;
            case 9: p.setTimestamp(i, Timestamp.valueOf(Frame.str(in).replace('T', ' '))); break;
            default: throw new IOException("bad param tag");
        }
    }

    static void write(ResultSet r, int i, int type, String typeName, long lobLimit, Frame.Out o) throws Exception {
        DataOutputStream d = o.d;
        switch (type) {
            case Types.BOOLEAN: case Types.BIT: {
                boolean v = r.getBoolean(i);
                if (r.wasNull()) { d.writeByte(0); return; }
                d.writeByte(1); d.writeBoolean(v); return;
            }
            case Types.TINYINT: case Types.SMALLINT: case Types.INTEGER: case Types.BIGINT: {
                long v = r.getLong(i);
                if (r.wasNull()) { d.writeByte(0); return; }
                d.writeByte(2); d.writeLong(v); return;
            }
            case Types.REAL: case Types.FLOAT: case Types.DOUBLE: case ORACLE_BINARY_FLOAT: case ORACLE_BINARY_DOUBLE: {
                double v = r.getDouble(i);
                if (r.wasNull()) { d.writeByte(0); return; }
                d.writeByte(3); d.writeDouble(v); return;
            }
            case Types.DECIMAL: case Types.NUMERIC: {
                BigDecimal v = r.getBigDecimal(i);
                if (v == null) { d.writeByte(0); return; }
                d.writeByte(4); o.str(v.toPlainString()); return;
            }
            case Types.DATE: {
                Date v = r.getDate(i);
                if (v == null) { d.writeByte(0); return; }
                d.writeByte(7); o.str(v.toString()); return;
            }
            case Types.TIME: {
                Time v = r.getTime(i);
                if (v == null) { d.writeByte(0); return; }
                d.writeByte(8); o.str(v.toString()); return;
            }
            case Types.TIMESTAMP_WITH_TIMEZONE: case ORACLE_TIMESTAMPTZ: {
                OffsetDateTime v = r.getObject(i, OffsetDateTime.class);
                if (v == null) { d.writeByte(0); return; }
                d.writeByte(12); o.str(v.toString()); return;
            }
            case Types.TIMESTAMP: {
                // GBase 8s 精度不完整的 DATETIME 取回的是拼出来的日期，按文本返回才不失真
                String name = typeName.toLowerCase();
                if (name.contains(" to ") && !name.contains("year to")) break;
                Timestamp v = r.getTimestamp(i);
                if (v == null) { d.writeByte(0); return; }
                d.writeByte(9); o.str(v.toLocalDateTime().format(TS)); return;
            }
            case Types.BINARY: case Types.VARBINARY: {
                byte[] v = r.getBytes(i);
                if (v == null) { d.writeByte(0); return; }
                d.writeByte(6); d.writeInt(v.length); d.write(v); return;
            }
            case Types.BLOB: case Types.LONGVARBINARY: {
                try (InputStream s = r.getBinaryStream(i)) {
                    if (s == null) { d.writeByte(0); return; }
                    writeBytes(s, lobLimit, o);
                }
                return;
            }
            case Types.CLOB: case Types.LONGVARCHAR: {
                try (Reader s = r.getCharacterStream(i)) {
                    if (s == null) { d.writeByte(0); return; }
                    writeText(s, lobLimit, o);
                }
                return;
            }
            default:
        }
        String v = r.getString(i);
        if (v == null) { d.writeByte(0); return; }
        d.writeByte(5); o.str(v);
    }

    private static byte[] readUpTo(InputStream s, long limit, long[] total) throws IOException {
        ByteArrayOutputStream head = new ByteArrayOutputStream();
        byte[] b = new byte[8192];
        for (int n; (n = s.read(b)) > 0; ) {
            total[0] += n;
            long room = limit - head.size();
            if (room > 0) head.write(b, 0, (int) Math.min(room, n));
        }
        return head.toByteArray();
    }

    private static void writeBytes(InputStream s, long limit, Frame.Out o) throws IOException {
        long[] total = {0};
        byte[] head = readUpTo(s, limit, total);
        if (total[0] <= limit) {
            o.d.writeByte(6); o.d.writeInt(head.length); o.d.write(head); return;
        }
        StringBuilder hex = new StringBuilder();
        for (byte x : head) hex.append(String.format("%02x", x));
        o.d.writeByte(10); o.d.writeBoolean(true); o.str(hex.toString()); o.d.writeLong(total[0]); o.d.writeBoolean(true);
    }

    /** 长度按 UTF-8 字节算，和 Rust 端的上限含义相同。 */
    private static void writeText(Reader s, long limit, Frame.Out o) throws IOException {
        StringBuilder head = new StringBuilder();
        long total = 0, kept = 0;
        char[] b = new char[8192];
        for (int n; (n = s.read(b)) > 0; ) {
            for (int i = 0; i < n; i++) {
                char c = b[i];
                int size = c < 0x80 ? 1 : c < 0x800 ? 2 : Character.isSurrogate(c) ? 2 : 3;
                total += size;
                if (kept + size <= limit) { head.append(c); kept += size; }
            }
        }
        if (total <= limit) { o.d.writeByte(5); o.str(head.toString()); return; }
        // 截断点不能落在代理对中间
        int end = head.length();
        if (end > 0 && Character.isHighSurrogate(head.charAt(end - 1))) head.setLength(end - 1);
        o.d.writeByte(10); o.d.writeBoolean(false); o.str(head.toString()); o.d.writeLong(total); o.d.writeBoolean(true);
    }

    private Values() {}
}
