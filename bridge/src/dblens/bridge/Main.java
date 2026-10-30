package dblens.bridge;

import java.io.*;
import java.net.*;
import java.nio.file.*;
import java.sql.*;
import java.util.*;
import java.util.concurrent.*;
import java.util.concurrent.atomic.AtomicLong;

/** 读帧线程：取消和分发在这里，耗时操作都到各连接自己的线程。stdin 关闭即退出。 */
public final class Main {
    private static final Map<Long, Session> sessions = new ConcurrentHashMap<>();
    private static final Map<String, java.sql.Driver> drivers = new ConcurrentHashMap<>();
    private static final AtomicLong nextId = new AtomicLong(1);
    private static final ExecutorService connector = Executors.newCachedThreadPool(r -> {
        Thread t = new Thread(r, "connect");
        t.setDaemon(true);
        return t;
    });

    public static void main(String[] args) throws Exception {
        DataInputStream in = new DataInputStream(new BufferedInputStream(System.in));
        DataOutputStream out = new DataOutputStream(new BufferedOutputStream(System.out));
        // 驱动的日志不能混进协议流
        System.setOut(System.err);
        for (Frame f; (f = Frame.read(in)) != null; ) dispatch(f, out);
        System.exit(0);
    }

    private static void dispatch(Frame f, DataOutputStream out) throws IOException {
        if (f.kind == 1) {
            connector.submit(() -> guarded(f, out, () -> connect(f, out)));
            return;
        }
        long conn = f.in.readLong();
        Session s = sessions.get(conn);
        if (s == null) {
            error(out, f.id, new SQLException("connection closed"));
            return;
        }
        if (f.kind == 4) {
            guarded(f, out, () -> { s.cancel(); ok(out, f.id); });
            return;
        }
        s.worker.submit(() -> guarded(f, out, () -> handle(f, s, out)));
    }

    private static void handle(Frame f, Session s, DataOutputStream out) throws Exception {
        switch (f.kind) {
            case 2: s.execute(f.id, f.in); return;
            case 3: s.fetch(f.id, f.in.readInt()); return;
            case 5: s.conn.commit(); break;
            case 6: s.conn.rollback(); break;
            case 7: s.conn.setAutoCommit(f.in.readBoolean()); break;
            case 8: if (!s.conn.isValid(5)) throw new SQLException("connection lost"); break;
            case 9: sessions.remove(s.id); s.close(); break;
            case 10: {
                DatabaseMetaData m = s.conn.getMetaData();
                Frame.Out o = new Frame.Out();
                o.str(m.getDatabaseProductName());
                o.str(m.getDatabaseProductVersion());
                o.send(out, f.id, 134);
                return;
            }
            case 11: s.setLobLimit(f.in.readLong()); break;
            case 12: s.endExec(); break;
            default: throw new IOException("bad request " + f.kind);
        }
        ok(out, f.id);
    }

    private static void connect(Frame f, DataOutputStream out) throws Exception {
        String cls = Frame.str(f.in), url = Frame.str(f.in), user = Frame.str(f.in), pw = Frame.str(f.in);
        String jarDir = Frame.str(f.in);
        int timeout = f.in.readInt();
        java.sql.Driver d = drivers.computeIfAbsent(jarDir + "|" + cls, k -> load(jarDir, cls));
        Properties p = new Properties();
        p.setProperty("user", user);
        p.setProperty("password", pw);
        DriverManager.setLoginTimeout(timeout);
        Connection c = d.connect(url, p);
        long id = nextId.getAndIncrement();
        sessions.put(id, new Session(id, c, out));
        Frame.Out o = new Frame.Out();
        o.d.writeLong(id);
        o.send(out, f.id, 129);
    }

    /** 驱动 jar 用独立的类加载器，这样不同版本的驱动互不影响。 */
    private static java.sql.Driver load(String jarDir, String cls) {
        try (DirectoryStream<Path> jars = Files.newDirectoryStream(Paths.get(jarDir), "*.jar")) {
            List<URL> urls = new ArrayList<>();
            for (Path j : jars) urls.add(j.toUri().toURL());
            ClassLoader cl = new URLClassLoader(urls.toArray(new URL[0]), Main.class.getClassLoader());
            return (java.sql.Driver) Class.forName(cls, true, cl).getDeclaredConstructor().newInstance();
        } catch (Exception e) {
            throw new IllegalStateException("cannot load " + cls + " from " + jarDir + ": " + e, e);
        }
    }

    private interface Body { void run() throws Exception; }

    private static void guarded(Frame f, DataOutputStream out, Body b) {
        try {
            b.run();
        } catch (Throwable t) {
            try { error(out, f.id, t); } catch (IOException e) { System.exit(1); }
        }
    }

    private static void ok(DataOutputStream out, long id) throws IOException {
        new Frame.Out().send(out, id, 128);
    }

    private static void error(DataOutputStream out, long id, Throwable t) throws IOException {
        Frame.Out o = new Frame.Out();
        SQLException s = t instanceof SQLException ? (SQLException) t : null;
        o.d.writeInt(s == null ? 0 : s.getErrorCode());
        o.str(s == null || s.getSQLState() == null ? "" : s.getSQLState());
        o.str(String.valueOf(t.getMessage()));
        o.send(out, id, 133);
    }
}
