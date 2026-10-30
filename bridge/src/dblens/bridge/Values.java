package dblens.bridge;

import java.io.*;
import java.math.BigDecimal;
import java.nio.charset.StandardCharsets;
import java.sql.*;
import java.time.format.DateTimeFormatter;

/** JDBC 值与线上值的转换。值的标记见 Frame。 */
final class Values {
    private static final DateTimeFormatter TS = DateTimeFormatter.ofPattern("yyyy-MM-dd'T'HH:mm:ss.SSSSSS");

    static void bind(PreparedStatement p, int i, DataInputStream in) throws Exception {
        switch (in.readUnsignedByte()) {
            case 0: p.setObject(i, null); break;
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
            case Types.SMALLINT: case Types.INTEGER: case Types.BIGINT: {
                long v = r.getLong(i);
                if (r.wasNull()) { d.writeByte(0); return; }
                d.writeByte(2); d.writeLong(v); return;
            }
            case Types.REAL: case Types.FLOAT: case Types.DOUBLE: {
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
            case Types.TIMESTAMP: {
                // 精度不完整的 DATETIME 取回的是拼出来的日期，按文本返回才不失真
                if (!typeName.toLowerCase().contains("year to")) break;
                Timestamp v = r.getTimestamp(i);
                if (v == null) { d.writeByte(0); return; }
                d.writeByte(9); o.str(v.toLocalDateTime().format(TS)); return;
            }
            case Types.BLOB: case Types.BINARY: case Types.VARBINARY: case Types.LONGVARBINARY: {
                try (InputStream s = r.getBinaryStream(i)) {
                    if (s == null) { d.writeByte(0); return; }
                    writeBytes(s, lobLimit, o);
                }
                return;
            }
            case Types.CLOB: case Types.LONGVARCHAR: {
                try (InputStream s = r.getBinaryStream(i)) {
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

    private static void writeText(InputStream s, long limit, Frame.Out o) throws IOException {
        long[] total = {0};
        byte[] head = readUpTo(s, limit, total);
        String text = new String(head, StandardCharsets.UTF_8);
        if (total[0] <= limit) { o.d.writeByte(5); o.str(text); return; }
        // 截断点可能落在多字节字符中间，去掉末尾的替换符
        if (text.endsWith("\uFFFD")) text = text.substring(0, text.length() - 1);
        o.d.writeByte(10); o.d.writeBoolean(false); o.str(text); o.d.writeLong(total[0]); o.d.writeBoolean(true);
    }

    private Values() {}
}
