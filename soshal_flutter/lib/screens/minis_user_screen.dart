import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:provider/provider.dart';
import 'package:video_player/video_player.dart';
import '../services/media_service.dart';
import '../services/minis_service.dart';
import '../services/p2p_service.dart';
import '../widgets/blob_image.dart';
import '../widgets/user_content_list.dart';

/// Minis user page: lists fetched minis. Mini events carry the author's
/// pubkey, so this shows the author's own published minis.
class MinisUserScreen extends StatefulWidget {
  /// Author pubkey being viewed.
  final String pubkey;

  /// Minis user screen.
  const MinisUserScreen({super.key, required this.pubkey});

  @override
  State<MinisUserScreen> createState() => _MinisUserScreenState();
}

class _MinisUserScreenState extends State<MinisUserScreen> {
  List<MiniItem> _minis = [];
  bool _loading = true;

  @override
  void initState() {
    super.initState();
    _load();
  }

  Future<void> _load() async {
    setState(() => _loading = true);
    try {
      _minis = context.read<MinisService>().fetchMinis();
    } catch (e) {
      debugPrint('minis user load: $e');
    }
    if (mounted) setState(() => _loading = false);
  }

  Future<void> _openMini(MiniItem mini) async {
    final url = await resolveMiniPlaybackUrl(
      mini,
      context.read<MediaService>(),
      context.read<P2pService>(),
    );
    if (!mounted) return;
    if (url == null) {
      ScaffoldMessenger.of(context).showSnackBar(
        const SnackBar(
            content: Text('Mini unavailable — no device has this blob.')),
      );
      return;
    }
    if (mini.videoUrl.startsWith('http') && url == mini.videoUrl) {
      showModalBottomSheet<void>(
        context: context,
        builder: (sheetContext) => SafeArea(
          child: Padding(
            padding: const EdgeInsets.all(16),
            child: Column(
              mainAxisSize: MainAxisSize.min,
              crossAxisAlignment: CrossAxisAlignment.stretch,
              children: [
                Text('Mini',
                    style: Theme.of(sheetContext).textTheme.titleLarge),
                const SizedBox(height: 8),
                Text(
                  mini.videoUrl,
                  maxLines: 2,
                  overflow: TextOverflow.ellipsis,
                  style: Theme.of(sheetContext).textTheme.bodyMedium,
                ),
                const SizedBox(height: 16),
                FilledButton.icon(
                  onPressed: () {
                    Clipboard.setData(ClipboardData(text: mini.videoUrl));
                    Navigator.of(sheetContext).pop();
                    if (!context.mounted) return;
                    ScaffoldMessenger.of(context).showSnackBar(
                      const SnackBar(
                          content: Text('Mini URL copied to clipboard')),
                    );
                  },
                  icon: const Icon(Icons.open_in_new),
                  label: const Text('Open mini'),
                ),
              ],
            ),
          ),
        ),
      );
      return;
    }
    await showDialog<void>(
      context: context,
      builder: (_) => _MiniVideoPlayer(url),
    );
  }

  @override
  Widget build(BuildContext context) {
    return UserContentList(
      title: 'Minis',
      loading: _loading,
      onRefresh: _load,
      emptyIcon: Icons.video_library,
      emptyTitle: 'No minis yet',
      emptyBody:
          'No minis available. Mini videos are hosted from device caches.',
      itemCount: _minis.length,
      itemBuilder: (context, index) {
        final mini = _minis[index];
        return ListTile(
          leading: mini.thumbnail.isNotEmpty
              ? ClipRRect(
                  borderRadius: BorderRadius.circular(8),
                  child: BlobImage(
                    source: mini.thumbnail,
                    width: 48,
                    height: 48,
                    fit: BoxFit.cover,
                    errorBuilder: (_) => Icon(Icons.video_library,
                        color: Theme.of(context).colorScheme.primary),
                  ),
                )
              : Icon(Icons.video_library,
                  color: Theme.of(context).colorScheme.primary),
          title: Text(
            mini.textOverlay.isEmpty
                ? (mini.videoUrl.startsWith('blob://')
                    ? 'Mini video'
                    : mini.videoUrl)
                : mini.textOverlay,
            maxLines: 1,
            overflow: TextOverflow.ellipsis,
          ),
          subtitle: Text(
            'Mini video · published by ${widget.pubkey}',
            maxLines: 1,
            overflow: TextOverflow.ellipsis,
          ),
          trailing: const Icon(Icons.play_circle_outline),
          onTap: () => _openMini(mini),
        );
      },
    );
  }
}

/// Plays a mini video (local blob-server URL or remote fallback) via
/// video_player inside a dialog.
class _MiniVideoPlayer extends StatefulWidget {
  final String url;

  const _MiniVideoPlayer(this.url);

  @override
  State<_MiniVideoPlayer> createState() => _MiniVideoPlayerState();
}

class _MiniVideoPlayerState extends State<_MiniVideoPlayer> {
  VideoPlayerController? _controller;
  String? _error;

  @override
  void initState() {
    super.initState();
    _controller = VideoPlayerController.networkUrl(Uri.parse(widget.url))
      ..initialize().then((_) {
        if (!mounted) return;
        setState(() {});
        _controller!.play();
      }).catchError((e) {
        if (!mounted) return;
        setState(() => _error = 'Playback failed: $e');
      });
  }

  @override
  void dispose() {
    _controller?.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    return AlertDialog(
      title: const Text('Mini'),
      content: SizedBox(
        width: 360,
        height: 240,
        child: _error != null
            ? Center(
                child: Text(_error!, style: const TextStyle(color: Colors.red)),
              )
            : _controller != null && _controller!.value.isInitialized
                ? FittedBox(
                    fit: BoxFit.contain,
                    child: SizedBox(
                      width: _controller!.value.size.width,
                      height: _controller!.value.size.height,
                      child: VideoPlayer(_controller!),
                    ),
                  )
                : const Center(child: CircularProgressIndicator()),
      ),
      actions: [
        TextButton(
          onPressed: () => Navigator.of(context).pop(),
          child: const Text('Close'),
        ),
      ],
    );
  }
}
