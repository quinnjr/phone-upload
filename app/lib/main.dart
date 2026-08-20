import 'dart:io';

import 'package:bonsoir/bonsoir.dart';
import 'package:device_info_plus/device_info_plus.dart';
import 'package:flutter/material.dart';
import 'package:path_provider/path_provider.dart';
import 'package:share_plus/share_plus.dart';

void main() {
  runApp(const PhoneDropApp());
}

class PhoneDropApp extends StatelessWidget {
  const PhoneDropApp({super.key});

  @override
  Widget build(BuildContext context) {
    return MaterialApp(
      title: 'Phone Drop',
      theme: ThemeData(
        colorScheme: ColorScheme.fromSeed(seedColor: Colors.teal),
        useMaterial3: true,
      ),
      home: const ReceiverPage(),
    );
  }
}

class ReceivedFile {
  ReceivedFile(this.file, this.receivedAt);

  final File file;
  final DateTime receivedAt;
}

class ReceiverPage extends StatefulWidget {
  const ReceiverPage({super.key});

  @override
  State<ReceiverPage> createState() => _ReceiverPageState();
}

class _ReceiverPageState extends State<ReceiverPage> {
  HttpServer? _server;
  BonsoirBroadcast? _broadcast;
  Directory? _saveDir;
  String? _error;
  String _deviceName = 'Phone';
  final List<ReceivedFile> _received = [];

  @override
  void initState() {
    super.initState();
    _start();
  }

  Future<void> _start() async {
    try {
      _saveDir = await getDownloadsDirectory() ??
          await getApplicationDocumentsDirectory();
      await _saveDir!.create(recursive: true);
      _loadExisting();

      _deviceName = await _lookupDeviceName();

      final server = await HttpServer.bind(InternetAddress.anyIPv4, 0);
      server.listen(_handleRequest, onError: (Object e) {
        setState(() => _error = e.toString());
      });
      _server = server;

      final broadcast = BonsoirBroadcast(
        service: BonsoirService(
          name: '$_deviceName Phone Drop',
          type: '_phoneupload._tcp',
          port: server.port,
        ),
      );
      await broadcast.initialize();
      await broadcast.start();
      _broadcast = broadcast;

      if (mounted) setState(() {});
    } catch (e) {
      if (mounted) setState(() => _error = e.toString());
    }
  }

  Future<String> _lookupDeviceName() async {
    try {
      final info = DeviceInfoPlugin();
      if (Platform.isAndroid) return (await info.androidInfo).model;
      if (Platform.isIOS) return (await info.iosInfo).name;
    } catch (_) {}
    return 'Phone';
  }

  void _loadExisting() {
    final files = _saveDir!
        .listSync()
        .whereType<File>()
        .map((f) => ReceivedFile(f, f.statSync().modified))
        .toList()
      ..sort((a, b) => b.receivedAt.compareTo(a.receivedAt));
    _received
      ..clear()
      ..addAll(files);
  }

  Future<void> _handleRequest(HttpRequest request) async {
    try {
      if (request.method != 'PUT') {
        request.response
          ..statusCode = HttpStatus.methodNotAllowed
          ..write('only PUT /?name=<filename> is supported\n');
        await request.response.close();
        return;
      }

      final rawName = request.uri.queryParameters['name'] ?? '';
      final name = _sanitize(rawName);
      if (name.isEmpty) {
        request.response
          ..statusCode = HttpStatus.badRequest
          ..write('missing ?name=<filename>\n');
        await request.response.close();
        return;
      }

      final target = _uniquePath(name);
      final sink = target.openWrite();
      try {
        await sink.addStream(request);
        await sink.flush();
      } finally {
        await sink.close();
      }

      request.response
        ..statusCode = HttpStatus.ok
        ..write('saved ${target.path}\n');
      await request.response.close();

      if (mounted) {
        setState(() {
          _received.insert(0, ReceivedFile(target, DateTime.now()));
        });
      }
    } catch (e) {
      try {
        request.response.statusCode = HttpStatus.internalServerError;
        request.response.write('error: $e\n');
        await request.response.close();
      } catch (_) {}
    }
  }

  /// Keeps only the basename so a malicious name can't escape the save dir.
  String _sanitize(String name) {
    final base = name.split(RegExp(r'[/\\]')).last.trim();
    if (base == '.' || base == '..') return '';
    return base;
  }

  File _uniquePath(String name) {
    var candidate = File('${_saveDir!.path}/$name');
    if (!candidate.existsSync()) return candidate;
    final dot = name.lastIndexOf('.');
    final stem = dot > 0 ? name.substring(0, dot) : name;
    final ext = dot > 0 ? name.substring(dot) : '';
    var i = 1;
    while (candidate.existsSync()) {
      candidate = File('${_saveDir!.path}/$stem ($i)$ext');
      i++;
    }
    return candidate;
  }

  String _formatSize(int bytes) {
    if (bytes < 1024) return '$bytes B';
    if (bytes < 1024 * 1024) return '${(bytes / 1024).toStringAsFixed(1)} KB';
    if (bytes < 1024 * 1024 * 1024) {
      return '${(bytes / (1024 * 1024)).toStringAsFixed(1)} MB';
    }
    return '${(bytes / (1024 * 1024 * 1024)).toStringAsFixed(2)} GB';
  }

  @override
  void dispose() {
    _broadcast?.stop();
    _server?.close(force: true);
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final server = _server;
    return Scaffold(
      appBar: AppBar(title: const Text('Phone Drop')),
      body: Column(
        children: [
          _StatusCard(
            error: _error,
            listening: server != null,
            port: server?.port,
            saveDir: _saveDir?.path,
          ),
          const Divider(height: 1),
          Expanded(
            child: _received.isEmpty
                ? const Center(
                    child: Text(
                      'No files yet.\nRun `phone-upload <file>` on your computer.',
                      textAlign: TextAlign.center,
                    ),
                  )
                : ListView.builder(
                    itemCount: _received.length,
                    itemBuilder: (context, index) {
                      final item = _received[index];
                      final exists = item.file.existsSync();
                      return ListTile(
                        leading: const Icon(Icons.insert_drive_file_outlined),
                        title: Text(item.file.uri.pathSegments.last),
                        subtitle: Text(exists
                            ? _formatSize(item.file.lengthSync())
                            : 'deleted'),
                        trailing: IconButton(
                          icon: const Icon(Icons.share),
                          onPressed: exists
                              ? () => SharePlus.instance.share(
                                    ShareParams(
                                      files: [XFile(item.file.path)],
                                    ),
                                  )
                              : null,
                        ),
                      );
                    },
                  ),
          ),
        ],
      ),
    );
  }
}

class _StatusCard extends StatelessWidget {
  const _StatusCard({
    required this.error,
    required this.listening,
    required this.port,
    required this.saveDir,
  });

  final String? error;
  final bool listening;
  final int? port;
  final String? saveDir;

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    return Padding(
      padding: const EdgeInsets.all(16),
      child: Row(
        children: [
          Icon(
            error != null
                ? Icons.error_outline
                : listening
                    ? Icons.wifi_tethering
                    : Icons.wifi_tethering_off,
            color: error != null
                ? theme.colorScheme.error
                : listening
                    ? Colors.green
                    : theme.disabledColor,
            size: 32,
          ),
          const SizedBox(width: 16),
          Expanded(
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Text(
                  error != null
                      ? 'Error: $error'
                      : listening
                          ? 'Discoverable on your network (port $port)'
                          : 'Starting…',
                  style: theme.textTheme.titleMedium,
                ),
                if (saveDir != null)
                  Text(
                    'Saving to $saveDir',
                    style: theme.textTheme.bodySmall,
                  ),
              ],
            ),
          ),
        ],
      ),
    );
  }
}
