import 'package:flutter/material.dart';
import 'package:provider/provider.dart';
import '../services/messaging_service.dart';
import '../services/session_service.dart';
import '../widgets/app_snack.dart';

/// Follow/Following toggle for someone else's profile. Hidden on the
/// viewer's own profile. Read state from the cached [ProfileInfo.isFollowing]
/// (kept fresh by [IdentityService.getProfile] after each toggle).
class FollowButton extends StatefulWidget {
  final String pubkey;

  const FollowButton({super.key, required this.pubkey});

  @override
  State<FollowButton> createState() => _FollowButtonState();
}

class _FollowButtonState extends State<FollowButton> {
  bool _busy = false;

  Future<void> _toggle() async {
    if (_busy) return;
    final identity = context.read<IdentityService>();
    final session = context.read<SessionService>();
    final me = session.activePubkey;
    if (me == null) {
      showAppSnack(context, 'Sign in to follow people');
      return;
    }
    if (me == widget.pubkey) return;
    setState(() => _busy = true);
    try {
      final following = context
              .read<IdentityService>()
              .profiles[widget.pubkey]
              ?.isFollowing ??
          false;
      if (following) {
        await identity.unfollowUser(widget.pubkey, me);
      } else {
        await identity.followUser(widget.pubkey, me);
      }
      await identity.getProfile(widget.pubkey, refresh: true);
      if (!mounted) return;
      final nowFollowing = context
              .read<IdentityService>()
              .profiles[widget.pubkey]
              ?.isFollowing ??
          false;
      showAppSnack(context, nowFollowing ? 'Following' : 'Unfollowed');
    } catch (e) {
      if (!mounted) return;
      showAppSnack(context, 'Follow error: $e');
    } finally {
      if (mounted) setState(() => _busy = false);
    }
  }

  @override
  Widget build(BuildContext context) {
    final session = context.watch<SessionService>();
    if (widget.pubkey == session.activePubkey) {
      return const SizedBox.shrink();
    }
    final identity = context.watch<IdentityService>();
    final following = identity.profiles[widget.pubkey]?.isFollowing ?? false;
    return ElevatedButton(
      onPressed: _busy || widget.pubkey.isEmpty ? null : _toggle,
      child: _busy
          ? const SizedBox(
              width: 16,
              height: 16,
              child: CircularProgressIndicator(strokeWidth: 2),
            )
          : Text(following ? 'Following' : 'Follow'),
    );
  }
}
