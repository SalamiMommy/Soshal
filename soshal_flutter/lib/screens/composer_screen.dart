// ignore_for_file: invalid_use_of_internal_member

import 'dart:async';
import 'dart:io';

import 'package:file_picker/file_picker.dart';
import 'package:flutter/material.dart';
import 'package:provider/provider.dart';
import '../services/audio_service.dart';
import '../services/feed_service.dart';
import '../services/media_service.dart';
import '../services/search_service.dart';
import '../services/session_service.dart';
import '../utils/format.dart';
import '../services/signer_service.dart';

/// Composer Screen
/// Create post, add media/tags
class ComposerScreen extends StatefulWidget {
  const ComposerScreen({super.key});

  @override
  State<ComposerScreen> createState() => _ComposerScreenState();
}

class _ComposerScreenState extends State<ComposerScreen> {
  final _contentController = TextEditingController();
  final List<String> _tags = [];
  final List<String> _mentions = [];
  Timer? _tagDebounce;
  List<String> _detectedTags = [];
  List<SearchResultItem> _mentionResults = [];
  static final RegExp _mentionRe = RegExp(r'@([A-Za-z0-9_.:\-]{1,})$');
  bool _isPosting = false;
  PostMedia? _pendingMedia;
  bool _uploadingMedia = false;
  double? _voiceDuration;
  List<double> _voicePeaks = const [];
  String? _voiceBlobHash;
  int _voiceSize = 0;

  /// Pick an audio file, encode it as a voice memo via the Rust storage-core
  /// codec, and attach it to the post as a blob-backed `audio` media tag.
  Future<void> _attachVoiceNote() async {
    try {
      final picked = await FilePicker.pickFile(
        type: FileType.any,
      );
      final path = picked?.path;
      if (path == null || !mounted) return;
      final audio = AudioService();
      final bytes = await File(path).readAsBytes();
      final pcm = bytes.length >= 44 && bytes[0] == 0x52 && bytes[8] == 0x57
          ? bytes.sublist(44)
          : bytes;
      if (pcm.isEmpty) throw Exception('No audio samples in $path');
      final payload = await audio.encodeVoice(pcm);
      final decoded = audio.decodeVoice(payload);
      if (decoded.isEmpty) throw Exception('Voice encode produced no samples');
      final duration = audio.durationSecs(payload);
      final peaks = await audio.peaksFor(path);
      final tmp = File(
          '${Directory.systemTemp.path}/voice_${DateTime.now().millisecondsSinceEpoch}.vo');
      await tmp.writeAsBytes(payload);
      if (!mounted) return;
      final manifest = await context.read<MediaService>().uploadMedia(tmp.path);
      final blobHash = manifest['blob_hash'] as String? ?? '';
      if (blobHash.length != 64) {
        throw Exception('Voice upload failed (bad manifest)');
      }
      if (!mounted) return;
      setState(() {
        _voiceDuration = duration;
        _voicePeaks = peaks;
        _voiceBlobHash = blobHash;
        _voiceSize = payload.length;
      });
    } catch (e) {
      if (mounted) {
        ScaffoldMessenger.of(context).showSnackBar(
          SnackBar(content: SelectableText('Voice note failed: $e')),
        );
      }
    }
  }

  void _onContentChanged() {
    _tagDebounce?.cancel();
    _tagDebounce = Timer(const Duration(milliseconds: 300), () async {
      final text = _contentController.text;
      final match = _mentionRe.firstMatch(text);
      final query = match?.group(1) ?? '';
      final resultsFuture = match == null
          ? Future.value(<SearchResultItem>[])
          : context
              .read<SearchService>()
              .mentions(query, limit: 8)
              .catchError((e) => <SearchResultItem>[]);
      final detected = context.read<FeedService>().utilExtractHashtags(text);
      final extracted = await Future.wait<Object?>([
        Future.value(detected),
        resultsFuture,
      ]);
      if (!mounted) return;
      setState(() => _detectedTags = extracted[0] as List<String>);
      if (match == null) {
        if (_mentionResults.isNotEmpty) {
          setState(() => _mentionResults = []);
        }
        return;
      }
      setState(() => _mentionResults = extracted[1] as List<SearchResultItem>);
    });
  }

  /// Insert a picked mention: drop the typed `@query` token and add the
  /// pubkey chip.
  void _selectMention(SearchResultItem r) {
    final key = r.pubkey ?? r.id;
    if (key.isEmpty) return;
    final text = _contentController.text;
    final match = _mentionRe.firstMatch(text);
    final start = match?.start ?? text.length;
    final next = text.substring(0, start);
    _contentController.text = next;
    _contentController.selection = TextSelection.collapsed(offset: next.length);
    setState(() {
      _mentions.add(key);
      _mentionResults = [];
    });
  }

  @override
  void dispose() {
    _tagDebounce?.cancel();
    _contentController.dispose();
    super.dispose();
  }

  Future<void> _attachMedia() async {
    try {
      final picked = await FilePicker.pickFile(
        type: FileType.any,
      );
      final path = picked?.path;
      if (path == null || !mounted) return;
      setState(() => _uploadingMedia = true);
      final manifest = await context.read<MediaService>().uploadMedia(path);
      final blobHash = manifest['blob_hash'] as String? ?? '';
      if (blobHash.length != 64) {
        throw Exception('Upload failed (bad manifest)');
      }
      final ext = path.split('.').last.toLowerCase();
      final type =
          const ['jpg', 'jpeg', 'png', 'gif', 'webp', 'heic'].contains(ext)
              ? 'image'
              : const ['mp4', 'mov', 'mkv', 'webm', 'avi'].contains(ext)
                  ? 'video'
                  : 'audio';
      if (!mounted) return;
      setState(() {
        _pendingMedia = PostMedia(
          url: 'blob://$blobHash',
          type: type,
          blobHash: blobHash,
          size: (manifest['total_size'] as num?)?.toInt() ?? 0,
        );
        _uploadingMedia = false;
      });
    } catch (e) {
      if (mounted) {
        setState(() => _uploadingMedia = false);
        ScaffoldMessenger.of(context).showSnackBar(
          SnackBar(content: SelectableText('Attach failed: $e')),
        );
      }
    }
  }

  Future<void> _publishPost() async {
    if (_isPosting) return;
    if (_contentController.text.isEmpty) {
      ScaffoldMessenger.of(context).showSnackBar(
        const SnackBar(content: SelectableText('Post content cannot be empty')),
      );
      return;
    }

    final text = _contentController.text;
    if (!context.read<FeedService>().validateNote(text)) {
      if (context.mounted) {
        ScaffoldMessenger.of(context).showSnackBar(
          const SnackBar(
              content: SelectableText('Note rejected by content validator')),
        );
      }
      return;
    }

    setState(() => _isPosting = true);
    try {
      if (await context.read<SignerService>().isLocked()) {
        if (mounted) {
          ScaffoldMessenger.of(context).showSnackBar(
            const SnackBar(
              content:
                  SelectableText('Signer locked — unlock in Security settings'),
            ),
          );
        }
        return;
      }
    } finally {
      setState(() => _isPosting = false);
    }
    if (!mounted) return;
    try {
      final feedService = context.read<FeedService>();
      final sessionService = context.read<SessionService>();

      if (sessionService.activePubkey == null) {
        throw Exception('No active account');
      }

      final tags = <List<String>>[
        ..._tags.map((tag) => ['t', tag]),
        ..._mentions.map((mention) => ['p', mention]),
        if (_pendingMedia != null)
          [
            'media',
            _pendingMedia!.type,
            _pendingMedia!.url,
            _pendingMedia!.blobHash,
            _pendingMedia!.size.toString(),
          ],
        if (_voiceBlobHash != null)
          [
            'media',
            'audio',
            'blob://$_voiceBlobHash',
            _voiceBlobHash!,
            _voiceSize.toString(),
          ],
      ];

      await feedService.publishTextNote(
        _contentController.text,
        tags,
        sessionService.activePubkey!,
      );

      // Mirror the published note into the offline outbox queue so the sync
      // engine can fan it out to additional relays.
      try {
        await feedService.enqueueOutboxPost(
          _contentController.text,
          mediaPath: _pendingMedia?.blobHash,
        );
      } catch (e) {
        debugPrint('outbox enqueue: $e');
      }

      if (mounted) {
        Navigator.of(context).pop();
        ScaffoldMessenger.of(context).showSnackBar(
          const SnackBar(content: SelectableText('Post published!')),
        );
      }
    } catch (e) {
      if (mounted) {
        ScaffoldMessenger.of(context).showSnackBar(
          SnackBar(content: SelectableText('Error: $e')),
        );
      }
    } finally {
      setState(() => _isPosting = false);
    }
  }

  @override
  Widget build(BuildContext context) {
    return Padding(
      padding: EdgeInsets.only(
        bottom: MediaQuery.of(context).viewInsets.bottom,
        left: 16,
        right: 16,
        top: 16,
      ),
      child: SingleChildScrollView(
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            // Header
            Row(
              mainAxisAlignment: MainAxisAlignment.spaceBetween,
              children: [
                const Text(
                  'New Post',
                  style: TextStyle(fontSize: 20, fontWeight: FontWeight.bold),
                ),
                IconButton(
                  icon: const Icon(Icons.close),
                  onPressed: () => Navigator.of(context).pop(),
                ),
              ],
            ),
            const SizedBox(height: 16),
            // Content input
            TextField(
              controller: _contentController,
              onChanged: (_) => _onContentChanged(),
              decoration: InputDecoration(
                hintText: 'What\'s happening?',
                border: OutlineInputBorder(
                  borderRadius: BorderRadius.circular(8),
                ),
              ),
              maxLines: 5,
              enabled: !_isPosting,
            ),
            const SizedBox(height: 8),
            if (_mentionResults.isNotEmpty)
              Material(
                elevation: 2,
                borderRadius: BorderRadius.circular(8),
                child: SizedBox(
                  height: 160,
                  child: ListView.builder(
                    shrinkWrap: true,
                    itemCount: _mentionResults.length,
                    itemBuilder: (context, i) {
                      final r = _mentionResults[i];
                      return ListTile(
                        dense: true,
                        leading: const Icon(Icons.person_outline, size: 18),
                        title: Text(r.title,
                            maxLines: 1, overflow: TextOverflow.ellipsis),
                        subtitle: Text(
                          firstChars(r.pubkey ?? r.id, 12),
                          style: const TextStyle(fontSize: 11),
                        ),
                        onTap: () => _selectMention(r),
                      );
                    },
                  ),
                ),
              ),
            const SizedBox(height: 8),
            if (_detectedTags.isNotEmpty)
              Wrap(
                spacing: 8,
                runSpacing: 4,
                children: _detectedTags.map((tag) {
                  final already = _tags.contains(tag);
                  return ActionChip(
                    avatar: already ? const Icon(Icons.check, size: 16) : null,
                    label: Text('#$tag'),
                    onPressed:
                        already ? null : () => setState(() => _tags.add(tag)),
                  );
                }).toList(),
              ),
            const SizedBox(height: 16),
            // Tags section
            if (_tags.isNotEmpty) ...[
              const Text(
                'Tags',
                style: TextStyle(fontWeight: FontWeight.bold),
              ),
              Wrap(
                spacing: 8,
                children: _tags.map((tag) {
                  return Chip(
                    label: Text('#$tag'),
                    onDeleted: () {
                      setState(() => _tags.remove(tag));
                    },
                  );
                }).toList(),
              ),
              const SizedBox(height: 16),
            ],
            // Mentions section
            if (_mentions.isNotEmpty) ...[
              const Text(
                'Mentions',
                style: TextStyle(fontWeight: FontWeight.bold),
              ),
              Wrap(
                spacing: 8,
                children: _mentions.map((mention) {
                  return Chip(
                    label:
                        Text('@${prefixEllipsis(mention, 8, ellipsis: '...')}'),
                    onDeleted: () {
                      setState(() => _mentions.remove(mention));
                    },
                  );
                }).toList(),
              ),
              const SizedBox(height: 16),
            ],
            // Action buttons
            Row(
              mainAxisAlignment: MainAxisAlignment.spaceEvenly,
              children: [
                IconButton(
                  icon: const Icon(Icons.tag),
                  onPressed: _isPosting ? null : _showTagDialog,
                  tooltip: 'Add tag',
                ),
                IconButton(
                  icon: const Icon(Icons.person_add),
                  onPressed: _isPosting ? null : _showMentionDialog,
                  tooltip: 'Mention user',
                ),
                IconButton(
                  icon: _uploadingMedia
                      ? const SizedBox(
                          height: 20,
                          width: 20,
                          child: CircularProgressIndicator(strokeWidth: 2),
                        )
                      : const Icon(Icons.attach_file),
                  onPressed:
                      _isPosting || _uploadingMedia ? null : _attachMedia,
                  tooltip: 'Attach media',
                ),
                IconButton(
                  icon: const Icon(Icons.mic),
                  onPressed:
                      _isPosting || _uploadingMedia ? null : _attachVoiceNote,
                  tooltip: 'Attach voice note (WAV / PCM)',
                ),
              ],
            ),
            if (_voiceBlobHash != null) ...[
              const SizedBox(height: 8),
              Chip(
                avatar: const Icon(Icons.mic),
                label: Text(
                  'Voice note · ${_voiceDuration?.toStringAsFixed(1)}s',
                ),
                onDeleted: () => setState(() {
                  _voiceDuration = null;
                  _voicePeaks = const [];
                  _voiceBlobHash = null;
                  _voiceSize = 0;
                }),
              ),
              if (_voicePeaks.isNotEmpty)
                Padding(
                  padding: const EdgeInsets.symmetric(vertical: 4),
                  child: Row(
                    mainAxisAlignment: MainAxisAlignment.center,
                    children: [
                      for (final p in _voicePeaks)
                        Container(
                          width: 3,
                          height: 4 + p.clamp(0.0, 1.0).toDouble() * 24,
                          margin: const EdgeInsets.symmetric(horizontal: 1),
                          decoration: BoxDecoration(
                            color: Theme.of(context).colorScheme.primary,
                            borderRadius: BorderRadius.circular(1),
                          ),
                        ),
                    ],
                  ),
                ),
            ],
            if (_pendingMedia != null) ...[
              const SizedBox(height: 8),
              Chip(
                avatar: Icon(
                  _pendingMedia!.type == 'image'
                      ? Icons.image
                      : _pendingMedia!.type == 'video'
                          ? Icons.videocam
                          : Icons.audiotrack,
                ),
                label: Text(
                  '${_pendingMedia!.type}: ${prefixEllipsis(_pendingMedia!.blobHash, 10)}',
                  overflow: TextOverflow.ellipsis,
                ),
                onDeleted: () => setState(() => _pendingMedia = null),
              ),
            ],
            const SizedBox(height: 16),
            // Publish button
            SizedBox(
              width: double.maxFinite,
              child: ElevatedButton(
                onPressed: _isPosting ? null : _publishPost,
                child: _isPosting
                    ? const SizedBox(
                        height: 20,
                        width: 20,
                        child: CircularProgressIndicator(strokeWidth: 2),
                      )
                    : const Text('Publish'),
              ),
            ),
            const SizedBox(height: 16),
          ],
        ),
      ),
    );
  }

  void _showTagDialog() {
    final controller = TextEditingController();
    showDialog(
      context: context,
      builder: (context) {
        return AlertDialog(
          title: const Text('Add Tag'),
          content: TextField(
            controller: controller,
            decoration: const InputDecoration(hintText: 'Tag name'),
          ),
          actions: [
            TextButton(
              onPressed: () => Navigator.of(context).pop(),
              child: const Text('Cancel'),
            ),
            TextButton(
              onPressed: () {
                if (controller.text.isNotEmpty) {
                  setState(() => _tags.add(controller.text));
                  Navigator.of(context).pop();
                }
              },
              child: const Text('Add'),
            ),
          ],
        );
      },
    );
  }

  void _showMentionDialog() {
    final controller = TextEditingController();
    List<SearchResultItem> results = [];
    showDialog(
      context: context,
      builder: (context) {
        return StatefulBuilder(
          builder: (context, setDialogState) {
            void onQuery(String q) async {
              final list = q.trim().isEmpty
                  ? <SearchResultItem>[]
                  : await context
                      .read<SearchService>()
                      .searchProfiles(q.trim(), limit: 8)
                      .catchError((e) => <SearchResultItem>[]);
              setDialogState(() => results = list);
            }

            return AlertDialog(
              title: const Text('Mention User'),
              content: Column(
                mainAxisSize: MainAxisSize.min,
                children: [
                  TextField(
                    controller: controller,
                    autofocus: true,
                    onChanged: onQuery,
                    decoration: InputDecoration(
                      hintText: 'Name, pubkey or npub',
                      suffixIcon: IconButton(
                        icon: const Icon(Icons.search),
                        onPressed: () => onQuery(controller.text),
                      ),
                    ),
                  ),
                  const SizedBox(height: 8),
                  if (results.isNotEmpty)
                    SizedBox(
                      height: 160,
                      child: ListView.builder(
                        shrinkWrap: true,
                        itemCount: results.length,
                        itemBuilder: (context, i) {
                          final r = results[i];
                          return ListTile(
                            dense: true,
                            leading: const Icon(Icons.person_outline),
                            title: Text(r.title,
                                maxLines: 1, overflow: TextOverflow.ellipsis),
                            subtitle: Text(
                              firstChars(r.pubkey ?? r.id, 12),
                              style: const TextStyle(fontSize: 11),
                            ),
                            onTap: () {
                              final key = r.pubkey ?? r.id;
                              if (key.isNotEmpty) {
                                setState(() => _mentions.add(key));
                                Navigator.of(context).pop();
                              }
                            },
                          );
                        },
                      ),
                    ),
                ],
              ),
              actions: [
                TextButton(
                  onPressed: () => Navigator.of(context).pop(),
                  child: const Text('Cancel'),
                ),
                TextButton(
                  onPressed: () {
                    if (controller.text.isNotEmpty) {
                      setState(() => _mentions.add(controller.text));
                      Navigator.of(context).pop();
                    }
                  },
                  child: const Text('Add'),
                ),
              ],
            );
          },
        );
      },
    );
  }
}
