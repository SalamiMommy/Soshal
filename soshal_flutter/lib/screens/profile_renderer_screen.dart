import 'dart:convert';

import 'package:flutter/material.dart';
import 'package:go_router/go_router.dart';
import 'package:provider/provider.dart';
import 'package:qr_flutter/qr_flutter.dart';
import '../ffi/auth.dart';
import '../ffi/db.dart' as ffi_db;
import '../models/custom_profile.dart';
import '../models/widget.dart';
import '../services/session_service.dart';

/// ProfileRendererScreen. Displays custom profile widgets with optional
/// drag-and-drop layout editing mode.
class ProfileRendererScreen extends StatefulWidget {
  final String pubkey;
  final bool initialEditMode;

  const ProfileRendererScreen({
    super.key,
    required this.pubkey,
    this.initialEditMode = false,
  });

  @override
  State<ProfileRendererScreen> createState() => _ProfileRendererScreenState();
}

class _ProfileRendererScreenState extends State<ProfileRendererScreen> {
  List<CustomProfileNode> _nodes = [];
  List<CustomProfileNode> _localNodes = [];
  bool _loading = true;
  bool _editMode = false;
  bool _saving = false;

  @override
  void initState() {
    super.initState();
    _editMode = widget.initialEditMode;
    _loadProfile();
  }

  Future<void> _loadProfile() async {
    setState(() => _loading = true);
    try {
      final profileData = ffi_db.dbGetCustomProfileNodes(pubkey: widget.pubkey);
      if (profileData.isNotEmpty) {
        try {
          final List<dynamic> nodesList =
              jsonDecode(profileData) as List<dynamic>;
          final loadedNodes = nodesList
              .map((e) => CustomProfileNode.fromJson(e as Map<String, dynamic>))
              .toList();
          setState(() {
            _nodes = loadedNodes;
            _localNodes = loadedNodes;
          });
        } catch (e) {
          debugPrint('Failed to parse profile data: $e');
        }
      }
    } catch (e) {
      debugPrint('ProfileRendererScreen: load error: $e');
      // Set default nodes if none exist
      setState(() {
        _nodes = [];
        _localNodes = [];
      });
    } finally {
      if (mounted) setState(() => _loading = false);
    }
  }

  void _toggleEditMode(bool enabled) {
    setState(() {
      _editMode = enabled;
      _localNodes = List.from(_nodes);
    });
  }

  void _handleDragEnd(List<CustomProfileNode> reordered) {
    setState(() => _localNodes = reordered);
  }

  void _handleNudgeNode(String nodeId, double dx, double dy) {
    setState(() {
      _localNodes = _localNodes.map((node) {
        if (node.id == nodeId) {
          final currentOffsetX = node.styles.offsetX ?? 0;
          final currentOffsetY = node.styles.offsetY ?? 0;
          return node.copyWith(
            styles: node.styles.copyWith(
              offsetX: currentOffsetX + dx,
              offsetY: currentOffsetY + dy,
            ),
          );
        }
        return node;
      }).toList();
    });
  }

  void _handleResizeNodeWidth(String nodeId, String width) {
    setState(() {
      _localNodes = _localNodes.map((node) {
        if (node.id == nodeId) {
          return node.copyWith(
            styles: node.styles.copyWith(width: width),
          );
        }
        return node;
      }).toList();
    });
  }

  void _handleDeleteWidget(String nodeId) {
    setState(
        () => _localNodes = _localNodes.where((n) => n.id != nodeId).toList());
  }

  Future<void> _handleSaveReorder() async {
    setState(() => _saving = true);
    try {
      final profile = CustomProfile(themeId: 'default', nodes: _localNodes);
      ffi_db.dbSaveCustomProfile(
        pubkey: widget.pubkey,
        profileJson: jsonEncode(profile.toJson()),
      );

      setState(() {
        _nodes = _localNodes;
        _editMode = false;
      });

      if (mounted) {
        ScaffoldMessenger.of(context).showSnackBar(
          const SnackBar(content: Text('Layout saved')),
        );
      }
    } catch (e) {
      debugPrint('ProfileRendererScreen: save error: $e');
      if (mounted) {
        ScaffoldMessenger.of(context).showSnackBar(
          SnackBar(content: Text('Failed to save: $e')),
        );
      }
    } finally {
      if (mounted) setState(() => _saving = false);
    }
  }

  void _showQrCode() {
    final npub = authNpubEncode(publicKey: widget.pubkey);
    showDialog<void>(
      context: context,
      builder: (dialogContext) => AlertDialog(
        title: const Text('Profile QR'),
        content: Column(
          mainAxisSize: MainAxisSize.min,
          children: [
            QrImageView(data: 'nostr:$npub', size: 220),
            const SizedBox(height: 16),
            SelectableText(npub, textAlign: TextAlign.center),
          ],
        ),
        actions: [
          TextButton(
            onPressed: () => Navigator.of(dialogContext).pop(),
            child: const Text('Close'),
          ),
        ],
      ),
    );
  }

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    final sessionService = context.read<SessionService>();
    final isMine = widget.pubkey == sessionService.activePubkey;

    if (_loading) {
      return Scaffold(
        appBar: AppBar(title: const Text('Profile')),
        body: const Center(child: CircularProgressIndicator()),
      );
    }

    if (_editMode && isMine) {
      return Scaffold(
        appBar: AppBar(
          title: const Text('Layout Editor'),
          elevation: 0,
          actions: [
            TextButton(
              onPressed: () => _toggleEditMode(false),
              child: const Text('Cancel'),
            ),
            TextButton(
              onPressed: _saving ? null : _handleSaveReorder,
              child: _saving
                  ? const SizedBox(
                      width: 16,
                      height: 16,
                      child: CircularProgressIndicator(strokeWidth: 2),
                    )
                  : const Text('Done'),
            ),
          ],
        ),
        body: Column(
          children: [
            Container(
              padding: const EdgeInsets.all(12),
              margin: const EdgeInsets.all(16),
              decoration: BoxDecoration(
                color: theme.colorScheme.surface,
                borderRadius: BorderRadius.circular(8),
                border: Border.all(color: theme.colorScheme.primary),
              ),
              child: Text(
                '⠿ Drag to reorder | ▲▼◀▶ Nudge offsets | ↔ Resize width',
                style: theme.textTheme.bodySmall?.copyWith(
                  color: theme.colorScheme.primary,
                  fontWeight: FontWeight.bold,
                ),
                textAlign: TextAlign.center,
              ),
            ),
            Expanded(
              child: _localNodes.isEmpty
                  ? Center(
                      child: Text(
                        'No widgets. Go back and use "Build My Profile" to add some.',
                        style: theme.textTheme.bodyMedium?.copyWith(
                          color: theme.colorScheme.onSurface
                              .withValues(alpha: 0.6),
                        ),
                        textAlign: TextAlign.center,
                      ),
                    )
                  : ReorderableListView.builder(
                      itemCount: _localNodes.length,
                      onReorder: (oldIndex, newIndex) {
                        if (newIndex > oldIndex) {
                          newIndex -= 1;
                        }
                        final reordered =
                            List<CustomProfileNode>.from(_localNodes);
                        final item = reordered.removeAt(oldIndex);
                        reordered.insert(newIndex, item);
                        _handleDragEnd(reordered);
                      },
                      itemBuilder: (context, index) {
                        final node = _localNodes[index];
                        return _EditableWidgetCard(
                          key: ValueKey(node.id),
                          node: node,
                          onNudge: _handleNudgeNode,
                          onResize: _handleResizeNodeWidth,
                          onDelete: _handleDeleteWidget,
                        );
                      },
                    ),
            ),
          ],
        ),
      );
    }

    return Scaffold(
      appBar: AppBar(
        title: const Text('Profile'),
        elevation: 0,
        actions: [
          IconButton(
            icon: const Icon(Icons.qr_code),
            onPressed: _showQrCode,
          ),
          if (isMine)
            _editMode
                ? Row(
                    mainAxisSize: MainAxisSize.min,
                    children: [
                      TextButton(
                        onPressed: () => _toggleEditMode(false),
                        child: const Text('Cancel'),
                      ),
                      TextButton(
                        onPressed: _saving ? null : _handleSaveReorder,
                        child: const Text('Done'),
                      ),
                    ],
                  )
                : TextButton.icon(
                    onPressed: () => _toggleEditMode(true),
                    icon: const Text('🎨'),
                    label: const Text('Edit Layout'),
                  ),
        ],
      ),
      body: SingleChildScrollView(
        child: Column(
          children: [
            const SizedBox(height: 16),
            if (_nodes.isEmpty) ...[
              const DefaultPostHistoryWidget(),
              if (isMine)
                Padding(
                  padding: const EdgeInsets.all(16.0),
                  child: ElevatedButton(
                    onPressed: () => context.push('/profile-builder'),
                    child: const Text('Build My Profile'),
                  ),
                ),
            ] else ...[
              for (final node in _nodes) _WidgetDisplayCard(node: node),
              if (!_nodes.any((n) => n.type == 'post_history'))
                const DefaultPostHistoryWidget(),
              if (!_nodes.any((n) => n.type == 'guestbook'))
                const DefaultGuestbookWidget(),
            ],
          ],
        ),
      ),
    );
  }
}

class _EditableWidgetCard extends StatelessWidget {
  final CustomProfileNode node;
  final Function(String, double, double) onNudge;
  final Function(String, String) onResize;
  final Function(String) onDelete;

  const _EditableWidgetCard({
    super.key,
    required this.node,
    required this.onNudge,
    required this.onResize,
    required this.onDelete,
  });

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    final currentWidth = node.styles.width ?? '100%';

    return Card(
      margin: const EdgeInsets.symmetric(horizontal: 16, vertical: 8),
      child: Column(
        children: [
          Container(
            padding: const EdgeInsets.all(8),
            decoration: BoxDecoration(
              color: theme.colorScheme.surface,
              border: Border.all(color: theme.colorScheme.outline),
              borderRadius:
                  const BorderRadius.vertical(top: Radius.circular(12)),
            ),
            child: Row(
              children: [
                ReorderableDragStartListener(
                  index: 0,
                  child: Container(
                    padding:
                        const EdgeInsets.symmetric(horizontal: 8, vertical: 4),
                    decoration: BoxDecoration(
                      color: theme.colorScheme.surfaceContainerHighest,
                      borderRadius: BorderRadius.circular(4),
                      border: Border.all(color: theme.colorScheme.outline),
                    ),
                    child: Text(
                      '⠿ Drag',
                      style: theme.textTheme.bodySmall?.copyWith(
                        fontWeight: FontWeight.bold,
                      ),
                    ),
                  ),
                ),
                const SizedBox(width: 8),
                // Nudge buttons
                Row(
                  children: [
                    _NudgeButton(
                      icon: Icons.arrow_upward,
                      onPressed: () => onNudge(node.id, 0, -10),
                    ),
                    _NudgeButton(
                      icon: Icons.arrow_downward,
                      onPressed: () => onNudge(node.id, 0, 10),
                    ),
                    _NudgeButton(
                      icon: Icons.arrow_back,
                      onPressed: () => onNudge(node.id, -10, 0),
                    ),
                    _NudgeButton(
                      icon: Icons.arrow_forward,
                      onPressed: () => onNudge(node.id, 10, 0),
                    ),
                  ],
                ),
                const SizedBox(width: 8),
                // Width resize buttons
                Row(
                  children: [
                    _WidthButton(
                      label: '50%',
                      isSelected: currentWidth == '50%',
                      onPressed: () => onResize(node.id, '50%'),
                    ),
                    _WidthButton(
                      label: '75%',
                      isSelected: currentWidth == '75%',
                      onPressed: () => onResize(node.id, '75%'),
                    ),
                    _WidthButton(
                      label: '100%',
                      isSelected:
                          currentWidth == '100%' || currentWidth == null,
                      onPressed: () => onResize(node.id, '100%'),
                    ),
                  ],
                ),
                const Spacer(),
                IconButton(
                  icon: const Icon(Icons.close),
                  color: theme.colorScheme.error,
                  onPressed: () => onDelete(node.id),
                ),
              ],
            ),
          ),
          Padding(
            padding: const EdgeInsets.all(16),
            child: _WidgetRenderer(node: node),
          ),
        ],
      ),
    );
  }
}

class _NudgeButton extends StatelessWidget {
  final IconData icon;
  final VoidCallback onPressed;

  const _NudgeButton({
    required this.icon,
    required this.onPressed,
  });

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    return Container(
      margin: const EdgeInsets.symmetric(horizontal: 2),
      child: IconButton(
        icon: Icon(icon, size: 16),
        onPressed: onPressed,
        padding: const EdgeInsets.all(4) as EdgeInsets?,
        constraints: const BoxConstraints(minWidth: 32, minHeight: 32),
        style: IconButton.styleFrom(
          backgroundColor: theme.colorScheme.surface,
          foregroundColor: theme.colorScheme.onSurface,
        ),
      ),
    );
  }
}

class _WidthButton extends StatelessWidget {
  final String label;
  final bool isSelected;
  final VoidCallback onPressed;

  const _WidthButton({
    required this.label,
    required this.isSelected,
    required this.onPressed,
  });

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    return Container(
      margin: const EdgeInsets.symmetric(horizontal: 2),
      child: OutlinedButton(
        onPressed: onPressed,
        style: OutlinedButton.styleFrom(
          backgroundColor: isSelected
              ? theme.colorScheme.primary
              : theme.colorScheme.surface,
          foregroundColor: isSelected
              ? theme.colorScheme.onPrimary
              : theme.colorScheme.onSurface,
          padding: const EdgeInsets.symmetric(horizontal: 8, vertical: 4),
          minimumSize: const Size(40, 32),
        ),
        child: Text(
          label,
          style: const TextStyle(fontSize: 10, fontWeight: FontWeight.bold),
        ),
      ),
    );
  }
}

class _WidgetDisplayCard extends StatelessWidget {
  final CustomProfileNode node;

  const _WidgetDisplayCard({required this.node});

  @override
  Widget build(BuildContext context) {
    return Card(
      margin: const EdgeInsets.symmetric(horizontal: 16, vertical: 8),
      child: Padding(
        padding: const EdgeInsets.all(16),
        child: _WidgetRenderer(node: node),
      ),
    );
  }
}

class _WidgetRenderer extends StatelessWidget {
  final CustomProfileNode node;

  const _WidgetRenderer({required this.node});

  @override
  Widget build(BuildContext context) {
    final typeInfo = nodeTypes.firstWhere(
      (t) => t.type == node.type,
      orElse: () => nodeTypes[0],
    );

    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        Row(
          children: [
            Text(typeInfo.icon, style: const TextStyle(fontSize: 24)),
            const SizedBox(width: 8),
            Text(
              node.properties['title'] as String? ?? typeInfo.label,
              style: Theme.of(context).textTheme.titleMedium,
            ),
          ],
        ),
        const SizedBox(height: 8),
        _renderWidgetContent(context, node),
      ],
    );
  }

  Widget _renderWidgetContent(BuildContext context, CustomProfileNode node) {
    switch (node.type) {
      case 'text_block':
        final props = TextBlockProperties.fromJson(node.properties);
        return Text(props.content);
      case 'media_gallery':
        final props = MediaGalleryProperties.fromJson(node.properties);
        if (props.items.isEmpty) {
          return const Text('No media items');
        }
        return GridView.builder(
          shrinkWrap: true,
          physics: const NeverScrollableScrollPhysics(),
          gridDelegate: SliverGridDelegateWithFixedCrossAxisCount(
            crossAxisCount: props.columns ?? 3,
            crossAxisSpacing: 4,
            mainAxisSpacing: 4,
          ),
          itemCount: props.items.length,
          itemBuilder: (context, index) {
            final item = props.items[index];
            return Image.network(item.url, fit: BoxFit.cover);
          },
        );
      case 'friend_grid':
        final props = FriendGridProperties.fromJson(node.properties);
        return Text('Top ${props.limit} friends');
      case 'music_player':
        final props = MusicPlayerProperties.fromJson(node.properties);
        if (props.tracks.isEmpty) {
          return const Text('No tracks');
        }
        return Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: props.tracks
              .map((track) => Text('• ${track.title} - ${track.artist}'))
              .toList(),
        );
      case 'contact_card':
        final props = ContactCardProperties.fromJson(node.properties);
        return Wrap(
          spacing: 8,
          children: [
            if (props.enableMessage) const Chip(label: Text('Message')),
            if (props.enableVouch) const Chip(label: Text('Vouch')),
            if (props.enableAddFriend) const Chip(label: Text('Add Friend')),
          ],
        );
      case 'qa_list':
        final props = QAListProperties.fromJson(node.properties);
        if (props.pairs.isEmpty) {
          return const Text('No Q&A pairs');
        }
        return Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: props.pairs
              .map((pair) => Padding(
                    padding: const EdgeInsets.symmetric(vertical: 4),
                    child: Column(
                      crossAxisAlignment: CrossAxisAlignment.start,
                      children: [
                        Text(pair.question,
                            style:
                                const TextStyle(fontWeight: FontWeight.bold)),
                        Text(pair.answer),
                      ],
                    ),
                  ))
              .toList(),
        );
      case 'guestbook':
        final props = GuestbookProperties.fromJson(node.properties);
        if (props.entries.isEmpty) {
          return const Text('No guestbook entries');
        }
        return Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: props.entries
              .map((entry) => Padding(
                    padding: const EdgeInsets.symmetric(vertical: 4),
                    child: Text('${entry.name}: ${entry.content}'),
                  ))
              .toList(),
        );
      case 'profile_links':
        final props = ProfileLinksProperties.fromJson(node.properties);
        return Wrap(
          spacing: 8,
          children: [
            if (props.showMinis) const Chip(label: Text('Minis')),
            if (props.showMusicloud) const Chip(label: Text('Musicloud')),
          ],
        );
      case 'post_history':
        return const DefaultPostHistoryWidget();
      case 'theme':
        final props = ThemeProperties.fromJson(node.properties);
        return Text('Theme: ${props.themeName}');
      case 'container':
      case 'tab_container':
      default:
        return const Text('Widget content');
    }
  }
}

class DefaultPostHistoryWidget extends StatelessWidget {
  const DefaultPostHistoryWidget({super.key});

  @override
  Widget build(BuildContext context) {
    return Card(
      margin: const EdgeInsets.symmetric(horizontal: 16, vertical: 8),
      child: Padding(
        padding: const EdgeInsets.all(16),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Row(
              children: [
                const Text('📜', style: TextStyle(fontSize: 24)),
                const SizedBox(width: 8),
                Text(
                  'Post History',
                  style: Theme.of(context).textTheme.titleMedium,
                ),
              ],
            ),
            const SizedBox(height: 8),
            const Text('Recent posts will appear here'),
          ],
        ),
      ),
    );
  }
}

class DefaultGuestbookWidget extends StatelessWidget {
  const DefaultGuestbookWidget({super.key});

  @override
  Widget build(BuildContext context) {
    return Card(
      margin: const EdgeInsets.symmetric(horizontal: 16, vertical: 8),
      child: Padding(
        padding: const EdgeInsets.all(16),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Row(
              children: [
                const Text('📖', style: TextStyle(fontSize: 24)),
                const SizedBox(width: 8),
                Text(
                  'Guestbook',
                  style: Theme.of(context).textTheme.titleMedium,
                ),
              ],
            ),
            const SizedBox(height: 8),
            const Text('Sign the guestbook'),
          ],
        ),
      ),
    );
  }
}
