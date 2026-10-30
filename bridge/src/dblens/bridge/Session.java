package dblens.bridge;

import java.io.*;
import java.sql.*;
import java.util.concurrent.*;

/** 一条数据库连接：请求在自己的线程上串行执行；取消由读帧线程直接调用，不排队。 */
final class Session {
    final long id;
    final Connection conn;
    final DataOutputStream out;
    final ExecutorService worker;
    volatile Statement current;
    private ResultSet rs;
    private int[] types;
    private String[] typeNames;
    private long lobLimit = 256 * 1024;

    Session(long id, Connection conn, DataOutputStream out) {
        this.id = id;
        this.conn = conn;
        this.out = out;
        this.worker = Executors.newSingleThreadExecutor(r -> {
            Thread t = new Thread(r, "session-" + id);
            t.setDaemon(true);
            return t;
        });
    }

    void execute(long req, DataInputStream in) throws Exception {
        endExec();
        String sql = Frame.str(in);
        int n = in.readInt();
        PreparedStatement p = conn.prepareStatement(sql);
        current = p;
        for (int i = 1; i <= n; i++) Values.bind(p, i, in);
        boolean hasRs = p.execute();
        Frame.Out o = new Frame.Out();
        if (!hasRs) {
            o.d.writeLong(Math.max(p.getUpdateCount(), 0));
            current = null;
            p.close();
            o.send(out, req, 131);
            return;
        }
        rs = p.getResultSet();
        ResultSetMetaData m = rs.getMetaData();
        int c = m.getColumnCount();
        types = new int[c];
        typeNames = new String[c];
        o.d.writeInt(c);
        for (int i = 1; i <= c; i++) {
            types[i - 1] = m.getColumnType(i);
            typeNames[i - 1] = m.getColumnTypeName(i);
            o.str(m.getColumnLabel(i));
            o.str(typeNames[i - 1]);
            int nullable = m.isNullable(i);
            o.d.writeByte(nullable == ResultSetMetaData.columnNoNulls ? 0 : nullable == ResultSetMetaData.columnNullable ? 1 : 2);
            o.d.writeByte(1); o.d.writeInt(m.getPrecision(i));
            o.d.writeByte(1); o.d.writeInt(m.getScale(i));
        }
        o.send(out, req, 130);
    }

    void fetch(long req, int maxRows) throws Exception {
        Frame.Out o = new Frame.Out();
        if (rs == null) {
            o.d.writeBoolean(true); o.d.writeInt(0);
            o.send(out, req, 132);
            return;
        }
        Frame.Out vals = new Frame.Out();
        int count = 0;
        boolean done = false;
        for (int row = 0; row < maxRows; row++) {
            if (!rs.next()) { done = true; break; }
            for (int i = 0; i < types.length; i++) {
                Values.write(rs, i + 1, types[i], typeNames[i], lobLimit, vals);
                count++;
            }
        }
        o.d.writeBoolean(done);
        o.d.writeInt(count);
        o.d.write(vals.buf.toByteArray());
        if (done) endExec();
        o.send(out, req, 132);
    }

    void endExec() {
        try { if (rs != null) rs.close(); } catch (SQLException ignored) { }
        try { if (current != null) current.close(); } catch (SQLException ignored) { }
        rs = null;
        current = null;
    }

    void setLobLimit(long bytes) { lobLimit = bytes; }

    void cancel() throws SQLException {
        Statement s = current;
        if (s != null) s.cancel();
    }

    void close() throws SQLException {
        endExec();
        conn.close();
        worker.shutdown();
    }
}
