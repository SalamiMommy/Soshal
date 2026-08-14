import 'package:flutter/material.dart';
import 'package:go_router/go_router.dart';
import 'package:provider/provider.dart';
import '../services/feed_service.dart';
import '../services/messaging_service.dart';
import '../services/session_service.dart';

/// Profile Page
/// Self and others, WoT indicators
class ProfileScreen extends StatefulWidget {
  final String? pubkey;

  const ProfileScreen({super.key, this.pubkey});

  @override
  State<ProfileScreen> createState() => _ProfileScreenState();
}

class _ProfileScreenState extends State<ProfileScreen> {
  bool _isFollowing = false;
  bool _isBlocked = false;
  bool _isLoading = true;
  List<FeedPost> _ownPosts = [];
  bool _postsLoading = true;
  String? _wotStatus;
  double? _wotScore;
  bool _wotUnavailable = false;
  Future<int>? _trustScoreFuture;

  @override
  void initState() {
    super.initState();
    _loadProfile();
  }

  Future<void> _loadProfile() async {
    try {
      final identityService = context.read<IdentityService>();
      final sessionService = context.read<SessionService>();
      final pubkey = widget.pubkey ?? sessionService.activePubkey;

      if (pubkey != null) {
        final me = sessionService.activePubkey;
        if (me != null && me != pubkey) {
          _trustScoreFuture = identityService.getTrustScore(pubkey, me);
        }
        if (widget.pubkey == null) {
          await identityService.getSelfProfile(pubkey);
        } else {
          await identityService.getProfile(pubkey);
        }
        if (!mounted) return;
        if (me != null && me != pubkey) {
          final blocked = await identityService.isBlocked(me, pubkey);
          if (mounted) setState(() => _isBlocked = blocked);
        }
        await _loadWot(pubkey, me ?? pubkey);
      }
      await _loadPosts(pubkey);
    } finally {
      if (mounted) setState(() => _isLoading = false);
    }
  }

  Future<void> _loadWot(String pubkey, String viewer) async {
    try {
      final identity = context.read<IdentityService>();
      final status = await identity.getWotStatus(pubkey, viewer);
      final score = await identity.getTrustScore(viewer, pubkey);
      if (!mounted) return;
      setState(() {
        _wotStatus = status;
        _wotScore = score / 100.0;
        _wotUnavailable = false;
      });
    } catch (e) {
      if (mounted) setState(() => _wotUnavailable = true);
    }
  }

  Future<void> _loadPosts(String? pubkey) async {
    try {
      final feed = context.read<FeedService>();
      await feed.loadPinnedPosts();
      final all = await feed.fetchWindow(limit: 200);
      if (!mounted) return;
      setState(() {
        _ownPosts = pubkey == null
            ? <FeedPost>[]
            : all.where((p) => p.pubkey == pubkey).toList();
        _postsLoading = false;
      });
    } catch (e) {
      if (mounted) setState(() => _postsLoading = false);
    }
  }

  Future<void> _pinToggle(String eventId) async {
    final feed = context.read<FeedService>();
    try {
      await feed.togglePin(eventId);
      if (mounted) setState(() {});
    } catch (e) {
      if (mounted) {
        ScaffoldMessenger.of(context)
            .showSnackBar(SnackBar(content: SelectableText('Pin error: $e')));
      }
    }
  }

  Future<void> _toggleFollow() async {
    final sessionService = context.read<SessionService>();
    final identity = context.read<IdentityService>();
    final me = sessionService.activePubkey;
    final target = widget.pubkey;
    if (me == null || target == null) return;
    try {
      if (_isFollowing) {
        await identity.unfollowUser(target, me);
      } else {
        await identity.followUser(target, me);
      }
      setState(() => _isFollowing = !_isFollowing);
    } catch (e) {
      if (mounted) {
        ScaffoldMessenger.of(context).showSnackBar(
            SnackBar(content: SelectableText('Follow error: $e')));
      }
    }
  }

  @override
  Widget build(BuildContext context) {
    final sessionService = context.read<SessionService>();
    final pubkey = widget.pubkey ?? sessionService.activePubkey;

    if (pubkey == null) {
      return Scaffold(
        appBar: AppBar(title: const Text('Profile')),
        body: const Center(child: Text('No profile loaded')),
      );
    }

    final isSelfProfile = pubkey == sessionService.activePubkey;

    return Scaffold(
      appBar: AppBar(
        title: const Text('Profile'),
        elevation: 0,
      ),
      body: Consumer<IdentityService>(
        builder: (context, identityService, child) {
          final feedService = context.watch<FeedService>();
          final profile = identityService.profiles[pubkey];

          if (_isLoading || profile == null) {
            return const Center(child: CircularProgressIndicator());
          }

          return SingleChildScrollView(
            child: Column(
              children: [
                // Banner
                Container(
                  height: 200,
                  color: Colors.grey[300],
                  child: profile.banner.isNotEmpty
                      ? Image.network(
                          profile.banner,
                          cacheHeight: 400,
                          fit: BoxFit.cover,
                        )
                      : null,
                ),
                // Avatar + bio
                Padding(
                  padding: const EdgeInsets.symmetric(horizontal: 16),
                  child: Column(
                    crossAxisAlignment: CrossAxisAlignment.start,
                    children: [
                      SizedBox(
                        height: 80,
                        child: Row(
                          crossAxisAlignment: CrossAxisAlignment.start,
                          children: [
                            Container(
                              margin: const EdgeInsets.only(top: -40),
                              child: CircleAvatar(
                                radius: 48,
                                backgroundImage: profile.picture.isNotEmpty
                                    ? ResizeImage.resizeIfNeeded(
                                        192, 192, NetworkImage(profile.picture))
                                    : null,
                              ),
                            ),
                            const Spacer(),
                            if (isSelfProfile)
                              ElevatedButton(
                                onPressed: () {
                                  context.go('/settings/edit-profile');
                                },
                                child: const Text('Edit Profile'),
                              )
                            else ...[
                              ElevatedButton(
                                onPressed: _toggleFollow,
                                child:
                                    Text(_isFollowing ? 'Following' : 'Follow'),
                              ),
                              IconButton(
                                icon: Icon(
                                  _isBlocked
                                      ? Icons.block
                                      : Icons.block_outlined,
                                  color: _isBlocked ? Colors.red : Colors.grey,
                                ),
                                tooltip: _isBlocked ? 'Unblock' : 'Block',
                                onPressed: () async {
                                  final session =
                                      context.read<SessionService>();
                                  final me = session.activePubkey;
                                  final target = widget.pubkey;
                                  if (me == null || target == null) return;
                                  try {
                                    if (_isBlocked) {
                                      await identityService.unblockUser(
                                          me, target);
                                    } else {
                                      await identityService.blockUser(
                                          me, target);
                                    }
                                    setState(() => _isBlocked = !_isBlocked);
                                    if (context.mounted) {
                                      ScaffoldMessenger.of(context)
                                          .showSnackBar(SnackBar(
                                        content: SelectableText(_isBlocked
                                            ? 'Blocked'
                                            : 'Unblocked'),
                                      ));
                                    }
                                  } catch (e) {
                                    if (context.mounted) {
                                      ScaffoldMessenger.of(context)
                                          .showSnackBar(SnackBar(
                                              content: Text('Error: $e')));
                                    }
                                  }
                                },
                              ),
                            ],
                          ],
                        ),
                      ),
                      const SizedBox(height: 8),
                      Text(
                        profile.displayName,
                        style: const TextStyle(
                          fontSize: 20,
                          fontWeight: FontWeight.bold,
                        ),
                      ),
                      if (profile.nip05.isNotEmpty)
                        Row(
                          children: [
                            Text(
                              profile.nip05,
                              style: const TextStyle(color: Colors.blue),
                            ),
                            if (profile.nip05Valid)
                              const Icon(Icons.verified,
                                  color: Colors.blue, size: 16),
                          ],
                        ),
                      Text(
                        pubkey.substring(0, 16),
                        style:
                            const TextStyle(color: Colors.grey, fontSize: 12),
                      ),
                      if (!isSelfProfile)
                        FutureBuilder<int>(
                          future: _trustScoreFuture,
                          builder: (context, snapshot) {
                            final score = snapshot.data;
                            return Padding(
                              padding: const EdgeInsets.only(top: 4),
                              child: Chip(
                                avatar: const Icon(Icons.workspace_premium,
                                    size: 16),
                                label: Text(
                                  score == null
                                      ? 'Trust: …'
                                      : 'Trust: ${score ~/ 100}.${(score % 100).toString().padLeft(2, '0')}',
                                ),
                                visualDensity: VisualDensity.compact,
                              ),
                            );
                          },
                        ),
                      const SizedBox(height: 8),
                      Text(profile.about),
                      const SizedBox(height: 16),
                      Row(
                        mainAxisAlignment: MainAxisAlignment.spaceEvenly,
                        children: [
                          _buildStat('${profile.followers}', 'Followers'),
                          _buildStat('${profile.following}', 'Following'),
                          _buildStat('WoT: ${profile.wotStatus}', 'Status'),
                        ],
                      ),
                      const SizedBox(height: 16),
                      if (!isSelfProfile)
                        ElevatedButton.icon(
                          onPressed: () {
                            context.go('/inbox/$pubkey');
                          },
                          icon: const Icon(Icons.message),
                          label: const Text('Message'),
                        ),
                      const SizedBox(height: 16),
                      if (isSelfProfile) ...[
                        const Divider(),
                        const SizedBox(height: 16),
                        _PinnedSection(
                          pinned: feedService.pinnedPosts,
                          onOpen: (id) => context.go('/post/$id'),
                          onUnpin: _pinToggle,
                        ),
                      ],
                      const SizedBox(height: 16),
                      const Divider(),
                      const SizedBox(height: 16),
                      _WotSection(
                        status: _wotStatus,
                        score: _wotScore,
                        unavailable: _wotUnavailable,
                        isSelf: isSelfProfile,
                      ),
                      const Divider(),
                      const SizedBox(height: 16),
                      const Text(
                        'Recent Posts',
                        style: TextStyle(
                          fontSize: 16,
                          fontWeight: FontWeight.bold,
                        ),
                      ),
                      const SizedBox(height: 16),
                      if (_postsLoading)
                        const Padding(
                          padding: EdgeInsets.symmetric(vertical: 24),
                          child: Center(child: CircularProgressIndicator()),
                        )
                      else if (_ownPosts.isEmpty)
                        const Padding(
                          padding: EdgeInsets.symmetric(vertical: 16),
                          child: Center(
                            child: Text(
                              'No posts yet',
                              style: TextStyle(color: Colors.grey),
                            ),
                          ),
                        )
                      else
                        for (final post in _ownPosts)
                          _PostCard(
                            post: post,
                            showPinButton: isSelfProfile,
                            pinned: feedService.isPinned(post.eventId),
                            onTap: () => context.go('/post/${post.eventId}'),
                            onPinToggle: () => _pinToggle(post.eventId),
                          ),
                    ],
                  ),
                ),
              ],
            ),
          );
        },
      ),
    );
  }

  Widget _buildStat(String value, String label) {
    return Column(
      children: [
        Text(value, style: const TextStyle(fontWeight: FontWeight.bold)),
        Text(label, style: const TextStyle(color: Colors.grey, fontSize: 12)),
      ],
    );
  }
}

/// Pinned posts section (own profile only): cards with Open/Unpin.
class _PinnedSection extends StatelessWidget {
  final List<String> pinned;
  final void Function(String id) onOpen;
  final void Function(String id) onUnpin;

  const _PinnedSection({
    required this.pinned,
    required this.onOpen,
    required this.onUnpin,
  });

  @override
  Widget build(BuildContext context) {
    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        const Text(
          'Pinned Posts',
          style: TextStyle(fontSize: 16, fontWeight: FontWeight.bold),
        ),
        const SizedBox(height: 8),
        if (pinned.isEmpty)
          const Text(
            'No pinned posts. Use 📌 Pin on your posts to pin them here.',
            style: TextStyle(color: Colors.grey),
          )
        else
          for (final id in pinned)
            Card(
              margin: const EdgeInsets.only(bottom: 8),
              child: Padding(
                padding: const EdgeInsets.all(12),
                child: Column(
                  crossAxisAlignment: CrossAxisAlignment.start,
                  children: [
                    Row(
                      children: [
                        const Text('📌 '),
                        Expanded(
                          child: SelectableText(
                            id,
                            style: const TextStyle(
                              fontSize: 12,
                              color: Colors.grey,
                            ),
                          ),
                        ),
                      ],
                    ),
                    const SizedBox(height: 8),
                    Row(
                      children: [
                        TextButton(
                          onPressed: () => onOpen(id),
                          child: const Text('Open'),
                        ),
                        const SizedBox(width: 8),
                        TextButton(
                          onPressed: () => onUnpin(id),
                          style: TextButton.styleFrom(
                            foregroundColor:
                                Theme.of(context).colorScheme.error,
                          ),
                          child: const Text('Unpin'),
                        ),
                      ],
                    ),
                  ],
                ),
              ),
            ),
      ],
    );
  }
}

/// Web-of-trust status + trust score (honest display of what the FFI returns).
class _WotSection extends StatelessWidget {
  final String? status;
  final double? score;
  final bool unavailable;
  final bool isSelf;

  const _WotSection({
    required this.status,
    required this.score,
    required this.unavailable,
    required this.isSelf,
  });

  @override
  Widget build(BuildContext context) {
    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        const Text(
          'Web of Trust',
          style: TextStyle(fontSize: 16, fontWeight: FontWeight.bold),
        ),
        const SizedBox(height: 8),
        if (unavailable)
          const Text(
            'Web of trust info unavailable.',
            style: TextStyle(color: Colors.grey),
          )
        else ...[
          if (score != null)
            Text('Trust score: ${(score! * 100).toStringAsFixed(2)}%'),
          if (status != null) ...[
            const SizedBox(height: 4),
            Text('Status: $status'
                '${status == 'trusted' ? ' (distance 1)' : status == 'warning' ? ' (distance 2)' : ''}'),
          ],
          const SizedBox(height: 4),
          const Text(
            'Distance 1 = trusted, distance 2 = warning.',
            style: TextStyle(color: Colors.grey, fontSize: 12),
          ),
        ],
      ],
    );
  }
}

/// A single post row on the profile feed with optional Pin/Unpin toggle.
class _PostCard extends StatelessWidget {
  final FeedPost post;
  final bool showPinButton;
  final bool pinned;
  final VoidCallback onTap;
  final VoidCallback onPinToggle;

  const _PostCard({
    required this.post,
    required this.showPinButton,
    required this.pinned,
    required this.onTap,
    required this.onPinToggle,
  });

  @override
  Widget build(BuildContext context) {
    return Card(
      margin: const EdgeInsets.only(bottom: 8),
      child: InkWell(
        onTap: onTap,
        borderRadius: BorderRadius.circular(12),
        child: Padding(
          padding: const EdgeInsets.all(12),
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              Text(
                post.content,
                maxLines: 3,
                overflow: TextOverflow.ellipsis,
              ),
              const SizedBox(height: 8),
              Row(
                children: [
                  Expanded(
                    child: Text(
                      post.eventId.length <= 12
                          ? post.eventId
                          : '${post.eventId.substring(0, 12)}…',
                      style: const TextStyle(color: Colors.grey, fontSize: 12),
                    ),
                  ),
                  if (showPinButton)
                    TextButton.icon(
                      onPressed: onPinToggle,
                      icon: Icon(
                        pinned ? Icons.push_pin : Icons.push_pin_outlined,
                        size: 16,
                      ),
                      label: Text(pinned ? 'Unpin' : 'Pin'),
                      style: TextButton.styleFrom(
                        visualDensity: VisualDensity.compact,
                      ),
                    ),
                ],
              ),
            ],
          ),
        ),
      ),
    );
  }
}
