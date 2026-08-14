import 'dart:convert';

import 'package:flutter/material.dart';
import 'package:go_router/go_router.dart';
import 'package:provider/provider.dart';
import '../services/marketplace_service.dart';
import '../services/session_service.dart';

/// Marketplace: listings, search, create, buy, orders and escrow.
class MarketplaceScreen extends StatefulWidget {
  /// Marketplace screen.
  const MarketplaceScreen({super.key});

  @override
  State<MarketplaceScreen> createState() => _MarketplaceScreenState();
}

class _MarketplaceScreenState extends State<MarketplaceScreen>
    with SingleTickerProviderStateMixin {
  late final TabController _tabs;
  bool _loading = true;
  final TextEditingController _search = TextEditingController();
  Future<List<ListingInfo>>? _sellerListingsFuture;

  @override
  void initState() {
    super.initState();
    _tabs = TabController(length: 3, vsync: this);
    _load();
  }

  @override
  void dispose() {
    _tabs.dispose();
    _search.dispose();
    super.dispose();
  }

  Future<void> _load() async {
    setState(() => _loading = true);
    try {
      final api = context.read<MarketplaceService>();
      final session = context.read<SessionService>();
      final q = _search.text.trim();
      if (session.activePubkey != null) {
        _sellerListingsFuture = api.sellerListings(session.activePubkey!);
      }
      if (q.isNotEmpty) {
        await api.search(q);
      } else {
        await api.fetchListings();
        if (session.activePubkey != null) {
          await api.sellerOrders(session.activePubkey!);
        }
      }
    } catch (e) {
      debugPrint('marketplace load: $e');
    }
    if (mounted) setState(() => _loading = false);
  }

  Future<String?> _createDialog() async {
    final title = TextEditingController();
    final desc = TextEditingController();
    final price = TextEditingController();
    final currency = TextEditingController(text: 'sats');
    final category = TextEditingController();
    final condition = TextEditingController(text: 'new');
    final images = TextEditingController();

    final ok = await showDialog<bool>(
      context: context,
      builder: (context) => AlertDialog(
        title: const Text('Create listing'),
        content: SingleChildScrollView(
          child: Column(
            mainAxisSize: MainAxisSize.min,
            children: [
              TextField(
                controller: title,
                decoration: const InputDecoration(labelText: 'Title *'),
              ),
              TextField(
                controller: desc,
                maxLines: 3,
                decoration: const InputDecoration(labelText: 'Description'),
              ),
              TextField(
                controller: price,
                keyboardType: TextInputType.number,
                decoration: const InputDecoration(labelText: 'Price *'),
              ),
              TextField(
                controller: currency,
                decoration: const InputDecoration(labelText: 'Currency'),
              ),
              TextField(
                controller: category,
                decoration: const InputDecoration(labelText: 'Category'),
              ),
              TextField(
                controller: condition,
                decoration: const InputDecoration(labelText: 'Condition'),
              ),
              TextField(
                controller: images,
                decoration: const InputDecoration(
                  labelText: 'Images (JSON array of URLs)',
                ),
              ),
            ],
          ),
        ),
        actions: [
          TextButton(
            onPressed: () => Navigator.pop(context, null),
            child: const Text('Cancel'),
          ),
          FilledButton(
            onPressed: () => Navigator.pop(context, true),
            child: const Text('Post'),
          ),
        ],
      ),
    );

    if (ok != true) return null;
    if (!mounted) return null;
    try {
      final session = context.read<SessionService>();
      final pubkey = session.activePubkey;
      if (pubkey == null) throw Exception('Sign in to sell');
      List<String> imageList = [];
      try {
        final parsed = images.text.trim();
        if (parsed.isNotEmpty) {
          imageList = (jsonDecode(parsed) as List<dynamic>)
              .map((e) => e.toString())
              .toList();
        }
      } catch (_) {
        imageList = images.text
            .split(',')
            .map((e) => e.trim())
            .where((e) => e.isNotEmpty)
            .toList();
      }
      final eventId = await context.read<MarketplaceService>().createListing(
            pubkey,
            title.text.trim(),
            desc.text.trim(),
            int.tryParse(price.text.trim()) ?? 0,
            currency.text.trim().isEmpty ? 'sats' : currency.text.trim(),
            category.text.trim(),
            condition.text.trim().isEmpty ? 'new' : condition.text.trim(),
            imageList,
            true,
          );
      return eventId;
    } catch (e) {
      if (mounted) {
        ScaffoldMessenger.of(context).showSnackBar(
            SnackBar(content: SelectableText('Create failed: $e')));
      }
      return null;
    }
  }

  Future<void> _listingDialog(ListingInfo listing) async {
    final session = context.read<SessionService>();
    final myPubkey = session.activePubkey ?? '';
    await showDialog<void>(
      context: context,
      builder: (context) => AlertDialog(
        title: Text(listing.title),
        content: SingleChildScrollView(
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.start,
            mainAxisSize: MainAxisSize.min,
            children: [
              Text(listing.priceLabel,
                  style: Theme.of(context).textTheme.titleMedium),
              const SizedBox(height: 8),
              if (listing.description.isNotEmpty) ...[
                Text(listing.description),
                const SizedBox(height: 8),
              ],
              _ListingDetailMeta(listing: listing, myPubkey: myPubkey),
              const Divider(height: 20),
              _EscrowSection(listing: listing, myPubkey: myPubkey),
            ],
          ),
        ),
        actions: [
          TextButton(
            onPressed: () => Navigator.pop(context),
            child: const Text('Close'),
          ),
          FilledButton(
            onPressed: () {
              Navigator.pop(context);
              _buy(listing);
            },
            child: const Text('Buy'),
          ),
        ],
      ),
    );
  }

  Future<void> _buy(ListingInfo listing) async {
    try {
      final session = context.read<SessionService>();
      final pubkey = session.activePubkey;
      if (pubkey == null) throw Exception('Sign in to buy');
      final api = context.read<MarketplaceService>();
      final orderId = await api.createOrder(
        listing.id,
        pubkey,
        listing.sellerPubkey,
      );
      if (mounted) {
        ScaffoldMessenger.of(context).showSnackBar(
          SnackBar(content: SelectableText('Order $orderId created')),
        );
        await _orderDialog(orderId, listing);
      }
    } catch (e) {
      if (mounted) {
        ScaffoldMessenger.of(context)
            .showSnackBar(SnackBar(content: SelectableText('Buy failed: $e')));
      }
    }
  }

  Future<void> _orderDialog(String orderId, ListingInfo listing) async {
    final session = context.read<SessionService>();
    final pubkey = session.activePubkey;
    if (pubkey == null) return;
    final api = context.read<MarketplaceService>();
    final escrow = await showDialog<String>(
      context: context,
      builder: (context) => AlertDialog(
        title: const Text('Escrow'),
        content: SelectableText(
          'Create an escrow for this order?\nAmount: ${listing.priceLabel}',
        ),
        actions: [
          TextButton(
            onPressed: () => Navigator.pop(context, 'skip'),
            child: const Text('Skip'),
          ),
          FilledButton(
            onPressed: () => Navigator.pop(context, 'escrow'),
            child: const Text('Create escrow'),
          ),
        ],
      ),
    );
    if (escrow == 'escrow') {
      try {
        final escrowId = await api.createEscrow(
          orderId,
          pubkey,
          listing.sellerPubkey,
          listing.price,
        );
        if (mounted) {
          ScaffoldMessenger.of(context).showSnackBar(
            SnackBar(content: SelectableText('Escrow $escrowId created')),
          );
        }
      } catch (e) {
        if (mounted) {
          ScaffoldMessenger.of(context).showSnackBar(
            SnackBar(content: SelectableText('Escrow failed: $e')),
          );
        }
      }
    }
  }

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      appBar: AppBar(
        title: TextField(
          controller: _search,
          decoration: const InputDecoration(
            hintText: 'Search listings',
            border: InputBorder.none,
          ),
          textInputAction: TextInputAction.search,
          onSubmitted: (_) => _load(),
        ),
        actions: [
          IconButton(
            icon: const Icon(Icons.search),
            onPressed: _load,
          ),
        ],
        bottom: TabBar(
          controller: _tabs,
          tabs: const [
            Tab(text: 'Browse'),
            Tab(text: 'Orders'),
            Tab(text: 'Mine'),
          ],
        ),
      ),
      floatingActionButton: FloatingActionButton(
        onPressed: () async {
          await _createDialog();
          await _load();
        },
        tooltip: 'Create listing',
        child: const Icon(Icons.add),
      ),
      body: _loading
          ? const Center(child: CircularProgressIndicator())
          : Column(
              children: [
                SizedBox(
                  height: 40,
                  child: ListView(
                    scrollDirection: Axis.horizontal,
                    padding: const EdgeInsets.symmetric(horizontal: 8),
                    children: [
                      for (final c in _categories)
                        Padding(
                          padding: const EdgeInsets.symmetric(horizontal: 4),
                          child: ChoiceChip(
                            label: Text(c.isEmpty ? 'All' : c),
                            selected: _category == c,
                            onSelected: (_) async {
                              _category = c;
                              setState(() {});
                              try {
                                final api = context.read<MarketplaceService>();
                                if (c.isEmpty) {
                                  await api.fetchListings();
                                } else {
                                  await api.byCategory(c);
                                }
                              } catch (e) {
                                debugPrint('category: $e');
                              }
                            },
                          ),
                        ),
                    ],
                  ),
                ),
                Expanded(
                  child: TabBarView(
                    controller: _tabs,
                    children: [
                      _buildBrowse(),
                      _buildOrders(),
                      _buildMine(),
                    ],
                  ),
                ),
              ],
            ),
    );
  }

  static const _categories = [
    '',
    'electronics',
    'handmade',
    'art',
    'books',
    'clothing',
    'tools',
    'food',
  ];

  String _category = '';

  Widget _buildBrowse() {
    return Consumer<MarketplaceService>(
      builder: (context, api, _) {
        if (api.listings.isEmpty) {
          return const Center(child: Text('No listings yet'));
        }
        return RefreshIndicator(
          onRefresh: _load,
          child: ListView.builder(
            itemExtent: 96.0,
            itemCount: api.listings.length,
            itemBuilder: (context, index) {
              final l = api.listings[index];
              return Card(
                margin: const EdgeInsets.symmetric(horizontal: 12, vertical: 6),
                child: ListTile(
                  leading: l.images.isNotEmpty
                      ? ClipRRect(
                          borderRadius: BorderRadius.circular(8),
                          child: Image.network(
                            l.images.first,
                            width: 56,
                            height: 56,
                            cacheWidth: 112,
                            cacheHeight: 112,
                            fit: BoxFit.cover,
                            errorBuilder: (_, __, ___) => const SizedBox(
                              width: 56,
                              height: 56,
                              child: Icon(Icons.inventory_2),
                            ),
                          ),
                        )
                      : const Icon(Icons.inventory_2),
                  title: Text(l.title,
                      maxLines: 1, overflow: TextOverflow.ellipsis),
                  subtitle: Column(
                    crossAxisAlignment: CrossAxisAlignment.start,
                    children: [
                      Text(
                        '${l.priceLabel} · ${l.sellerName.isEmpty ? l.sellerPubkey.substring(0, 8) : l.sellerName}'
                        '\n${l.category.isEmpty ? 'uncategorized' : l.category}',
                        maxLines: 2,
                        overflow: TextOverflow.ellipsis,
                      ),
                      if (l.condition.trim().isNotEmpty)
                        Padding(
                          padding: const EdgeInsets.only(top: 4),
                          child: _ConditionChip(condition: l.condition),
                        ),
                    ],
                  ),
                  isThreeLine: true,
                  trailing: FilledButton.tonal(
                    onPressed: () => _buy(l),
                    child: const Text('Buy'),
                  ),
                  onTap: () => _listingDialog(l),
                ),
              );
            },
          ),
        );
      },
    );
  }

  Widget _buildOrders() {
    return Consumer<MarketplaceService>(
      builder: (context, api, _) {
        if (api.orders.isEmpty) {
          return const Center(child: Text('No orders yet'));
        }
        return ListView.builder(
          itemExtent: 64.0,
          itemCount: api.orders.length,
          itemBuilder: (context, index) {
            final o = api.orders[index];
            return ListTile(
              leading: const Icon(Icons.receipt_long),
              title: Text('Order ${o.id.substring(0, 8)}'),
              subtitle: Text('${o.amount} sats · ${o.status}'),
              trailing: o.status == 'created'
                  ? TextButton(
                      onPressed: () async {
                        final session = context.read<SessionService>();
                        final pubkey = session.activePubkey;
                        if (pubkey == null) return;
                        try {
                          await api.disputeEscrow(
                              o.id, pubkey, 'buyer dispute');
                          if (context.mounted) {
                            ScaffoldMessenger.of(context).showSnackBar(
                              const SnackBar(
                                  content: SelectableText('Disputed')),
                            );
                          }
                        } catch (e) {
                          if (context.mounted) {
                            ScaffoldMessenger.of(context).showSnackBar(
                              SnackBar(
                                  content:
                                      SelectableText('Dispute failed: $e')),
                            );
                          }
                        }
                      },
                      child: const Text('Dispute'),
                    )
                  : null,
            );
          },
        );
      },
    );
  }

  Widget _buildMine() {
    return Consumer<MarketplaceService>(
      builder: (context, api, _) {
        final session = context.read<SessionService>();
        final pubkey = session.activePubkey;
        if (pubkey == null) {
          return const Center(child: Text('Sign in to see your listings'));
        }
        return FutureBuilder<List<ListingInfo>>(
          future: _sellerListingsFuture,
          builder: (context, snapshot) {
            final mine = snapshot.data ?? [];
            if (mine.isEmpty) {
              return const Center(child: Text('You have no listings'));
            }
            return ListView.builder(
              itemExtent: 64.0,
              itemCount: mine.length,
              itemBuilder: (context, index) {
                final l = mine[index];
                return ListTile(
                  leading: const Icon(Icons.storefront),
                  title: Text(l.title),
                  subtitle: Text(l.priceLabel),
                  trailing: IconButton(
                    icon: const Icon(Icons.delete_outline),
                    tooltip: 'Delete',
                    onPressed: () async {
                      try {
                        await api.deleteListing(l.id, pubkey);
                        setState(() {});
                      } catch (e) {
                        if (context.mounted) {
                          ScaffoldMessenger.of(context).showSnackBar(
                            SnackBar(
                                content: SelectableText('Delete failed: $e')),
                          );
                        }
                      }
                    },
                  ),
                );
              },
            );
          },
        );
      },
    );
  }
}

String _shortPk(String pk) {
  if (pk.length <= 12) return pk;
  return '${pk.substring(0, 8)}…${pk.substring(pk.length - 4)}';
}

/// Condition pill with the legacy color mapping (new/like new/good/fair).
class _ConditionChip extends StatelessWidget {
  const _ConditionChip({required this.condition});

  final String condition;

  @override
  Widget build(BuildContext context) {
    final raw = condition.trim().toLowerCase().replaceAll('_', ' ');
    final (color, label) = switch (raw) {
      'new' => (const Color(0xFF16a34a), 'New'),
      'like new' => (const Color(0xFF0d9488), 'Like New'),
      'good' => (const Color(0xFFd97706), 'Good'),
      'fair' => (const Color(0xFFdc2626), 'Fair'),
      _ when raw.isEmpty => (const Color(0xFF6b7280), ''),
      _ => (const Color(0xFF6b7280), condition),
    };
    if (label.isEmpty) return const SizedBox.shrink();
    return Container(
      padding: const EdgeInsets.symmetric(horizontal: 10, vertical: 3),
      decoration: BoxDecoration(
        color: color,
        borderRadius: BorderRadius.circular(999),
      ),
      child: Text(
        label,
        style: const TextStyle(
          color: Colors.white,
          fontSize: 12,
          fontWeight: FontWeight.w600,
        ),
      ),
    );
  }
}

/// Detail meta rows above the escrow panel: condition pill, 📍 geohash,
/// 🏷 tags (read from the listing's content JSON) and the seller contact row.
class _ListingDetailMeta extends StatefulWidget {
  const _ListingDetailMeta({required this.listing, required this.myPubkey});

  final ListingInfo listing;
  final String myPubkey;

  @override
  State<_ListingDetailMeta> createState() => _ListingDetailMetaState();
}

class _ListingDetailMetaState extends State<_ListingDetailMeta> {
  Map<String, dynamic> _content = const {};

  @override
  void initState() {
    super.initState();
    _load();
  }

  Future<void> _load() async {
    final content = await context
        .read<MarketplaceService>()
        .listingContent(widget.listing.id);
    if (!mounted) return;
    setState(() => _content = content);
  }

  String get _geohash => _content['locationGeohash'] as String? ?? '';
  List<String> get _tags => (_content['tags'] as List<dynamic>? ?? [])
      .map((e) => e.toString())
      .where((e) => e.isNotEmpty)
      .toList();

  @override
  Widget build(BuildContext context) {
    final listing = widget.listing;
    final seller = listing.sellerPubkey;
    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      mainAxisSize: MainAxisSize.min,
      children: [
        Wrap(
          spacing: 6,
          runSpacing: 4,
          crossAxisAlignment: WrapCrossAlignment.center,
          children: [
            _ConditionChip(condition: listing.condition),
            if (listing.category.isNotEmpty) _infoChip(listing.category),
          ],
        ),
        if (_geohash.isNotEmpty)
          Padding(
            padding: const EdgeInsets.only(top: 6),
            child: Text('📍 $_geohash',
                style: Theme.of(context).textTheme.bodySmall),
          ),
        if (_tags.isNotEmpty)
          Padding(
            padding: const EdgeInsets.only(top: 2),
            child: Text('🏷 ${_tags.join(', ')}',
                style: Theme.of(context).textTheme.bodySmall),
          ),
        const SizedBox(height: 10),
        Row(
          children: [
            Expanded(
              child: Text(
                'Seller: ${listing.sellerName.isEmpty ? _shortPk(seller) : listing.sellerName}'
                ' · ${_shortPk(seller)}',
                style: Theme.of(context).textTheme.bodySmall,
                maxLines: 1,
                overflow: TextOverflow.ellipsis,
              ),
            ),
            if (seller.isNotEmpty && seller != widget.myPubkey)
              FilledButton.tonal(
                onPressed: () => context.go('/inbox/$seller'),
                child: const Text('Send DM'),
              ),
          ],
        ),
      ],
    );
  }

  Widget _infoChip(String label) {
    return Container(
      padding: const EdgeInsets.symmetric(horizontal: 10, vertical: 3),
      decoration: BoxDecoration(
        border: Border.all(color: Theme.of(context).colorScheme.outline),
        borderRadius: BorderRadius.circular(999),
      ),
      child: Text(label, style: const TextStyle(fontSize: 12)),
    );
  }
}

/// NIP-15 escrow workflow: badge row + expandable state-machine panel.
/// States: created → disputed → completed/refunded (funded/shipped are
/// backend-gated in this port and never faked).
class _EscrowSection extends StatefulWidget {
  const _EscrowSection({required this.listing, required this.myPubkey});

  final ListingInfo listing;
  final String myPubkey;

  @override
  State<_EscrowSection> createState() => _EscrowSectionState();
}

class _EscrowSectionState extends State<_EscrowSection> {
  bool _checked = false;
  bool _escrowSupported = false;
  EscrowInfo? _escrow;
  bool _busy = false;

  MarketplaceService get _api => context.read<MarketplaceService>();

  @override
  void initState() {
    super.initState();
    _init();
  }

  Future<void> _init() async {
    final supported = await _api.supportsEscrow(widget.listing.id);
    if (!mounted) return;
    setState(() {
      _escrowSupported = supported;
      _checked = true;
    });
    await _refresh();
  }

  Future<void> _refresh() async {
    final escrow = await _api.getEscrowByListing(widget.listing.id);
    if (!mounted) return;
    setState(() => _escrow = escrow);
  }

  void _snack(String message) {
    if (!mounted) return;
    ScaffoldMessenger.of(context)
        .showSnackBar(SnackBar(content: SelectableText(message)));
  }

  /// Runs a transition, reports the result, then refetches escrow state.
  Future<void> _run(String okMsg, Future<void> Function() action) async {
    setState(() => _busy = true);
    try {
      await action();
      _snack(okMsg);
    } catch (e) {
      _snack('Escrow error: $e');
    }
    if (!mounted) return;
    try {
      final escrow = await _api.getEscrowByListing(widget.listing.id);
      if (!mounted) return;
      setState(() {
        _escrow = escrow;
        _busy = false;
      });
    } catch (e) {
      if (!mounted) return;
      setState(() => _busy = false);
    }
  }

  /// Order for this listing from the orders cache, falling back to a
  /// buyer-orders fetch. Null when none exists.
  Future<OrderInfo?> _findOrder(String listingId) async {
    final local = _api.orderForListing(listingId);
    if (local != null) return local;
    try {
      final orders = await _api.buyerOrders(widget.myPubkey);
      for (final o in orders) {
        if (o.listingId == listingId) return o;
      }
    } catch (_) {}
    return null;
  }

  Future<void> _fund() async {
    final order = await _findOrder(widget.listing.id);
    if (order == null) {
      _snack('An order is required to fund escrow — tap "Buy" on the '
          'listing to create one first.');
      return;
    }
    await _run('Escrow funded — ${_shortPk(order.id)}', () async {
      await _api.createEscrow(
        order.id,
        widget.myPubkey,
        widget.listing.sellerPubkey,
        widget.listing.price,
      );
    });
  }

  Future<void> _release(EscrowInfo escrow) => _run(
        'Escrow released — funds paid to seller ✓',
        () => _api.releaseEscrow(escrow.id, widget.listing.sellerPubkey),
      );

  Future<void> _disputeDialog(EscrowInfo escrow) async {
    final reason = TextEditingController();
    final ok = await showDialog<bool>(
      context: context,
      builder: (context) => AlertDialog(
        title: const Text('Open dispute'),
        content: TextField(
          controller: reason,
          maxLines: 3,
          autofocus: true,
          decoration: const InputDecoration(hintText: 'Reason for dispute'),
        ),
        actions: [
          TextButton(
            onPressed: () => Navigator.pop(context, false),
            child: const Text('Cancel'),
          ),
          FilledButton(
            onPressed: () => Navigator.pop(context, true),
            child: const Text('Confirm dispute'),
          ),
        ],
      ),
    );
    if (ok != true || !mounted) return;
    await _run('Dispute opened — mediator notified ⚠️', () async {
      await _api.disputeEscrow(escrow.id, widget.myPubkey, reason.text.trim());
    });
  }

  Future<void> _resolveDialog(EscrowInfo escrow) async {
    final winner = await showDialog<String>(
      context: context,
      builder: (context) => SimpleDialog(
        title: const Text('Resolve dispute'),
        children: [
          SimpleDialogOption(
            onPressed: () => Navigator.pop(context, escrow.buyerPubkey),
            child: Text('Resolve for buyer (${_shortPk(escrow.buyerPubkey)})'),
          ),
          SimpleDialogOption(
            onPressed: () => Navigator.pop(context, escrow.sellerPubkey),
            child:
                Text('Resolve for seller (${_shortPk(escrow.sellerPubkey)})'),
          ),
          SimpleDialogOption(
            onPressed: () => Navigator.pop(context),
            child: const Text('Cancel'),
          ),
        ],
      ),
    );
    if (winner == null || !mounted) return;
    final isBuyer = winner == escrow.buyerPubkey;
    await _run('Escrow resolved ${isBuyer ? 'for buyer' : 'for seller'} ✓',
        () async {
      await _api.resolveEscrow(escrow.id, widget.myPubkey, winner);
    });
  }

  String _shortPk(String pk) {
    if (pk.length <= 12) return pk;
    return '${pk.substring(0, 8)}…${pk.substring(pk.length - 4)}';
  }

  @override
  Widget build(BuildContext context) {
    if (!_checked) {
      return const Padding(
        padding: EdgeInsets.symmetric(vertical: 12),
        child: Center(
          child: SizedBox(
            width: 18,
            height: 18,
            child: CircularProgressIndicator(strokeWidth: 2),
          ),
        ),
      );
    }
    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      mainAxisSize: MainAxisSize.min,
      children: [
        if (_escrowSupported)
          Container(
            padding: const EdgeInsets.symmetric(horizontal: 10, vertical: 4),
            decoration: BoxDecoration(
              color: const Color(0xFF374151),
              borderRadius: BorderRadius.circular(999),
            ),
            child: const Row(
              mainAxisSize: MainAxisSize.min,
              children: [
                Text('🔒 ', style: TextStyle(fontSize: 12)),
                Text(
                  'Escrow',
                  style: TextStyle(
                    fontSize: 12,
                    fontWeight: FontWeight.w600,
                    color: Colors.white,
                  ),
                ),
              ],
            ),
          ),
        ExpansionTile(
          tilePadding: EdgeInsets.zero,
          leading: const Icon(Icons.lock_outline),
          title: const Text('Open Escrow Panel'),
          childrenPadding: const EdgeInsets.only(bottom: 8),
          children: [_buildPanel()],
        ),
      ],
    );
  }

  Widget _buildPanel() {
    final escrow = _escrow;
    final signedIn = widget.myPubkey.isNotEmpty;
    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      mainAxisSize: MainAxisSize.min,
      children: [
        if (escrow == null)
          const Padding(
            padding: EdgeInsets.only(bottom: 8),
            child: Text('No escrow for this listing yet.'),
          )
        else ...[
          Wrap(
            spacing: 6,
            runSpacing: 4,
            crossAxisAlignment: WrapCrossAlignment.center,
            children: [
              _statusChip(escrow.status),
              if (escrow.buyerPubkey == widget.myPubkey)
                _roleBadge('You are buyer'),
              if (escrow.sellerPubkey == widget.myPubkey)
                _roleBadge('You are seller'),
            ],
          ),
          const SizedBox(height: 6),
          Text('Amount: ${escrow.amountMsats} ${escrow.currency}',
              style: Theme.of(context).textTheme.bodyMedium),
          if (escrow.note.isNotEmpty)
            Padding(
              padding: const EdgeInsets.only(top: 2),
              child: Text('Note: ${escrow.note}',
                  style: Theme.of(context).textTheme.bodySmall),
            ),
          const SizedBox(height: 8),
        ],
        ..._stateActions(escrow, signedIn),
        if (!signedIn)
          Padding(
            padding: const EdgeInsets.only(top: 6),
            child: Text('Sign in to manage escrow',
                style: Theme.of(context).textTheme.bodySmall),
          ),
        if (_busy)
          const Padding(
            padding: EdgeInsets.only(top: 8),
            child: LinearProgressIndicator(),
          ),
      ],
    );
  }

  List<Widget> _stateActions(EscrowInfo? escrow, bool signedIn) {
    final enabled = signedIn && !_busy;
    final e = escrow;
    if (e == null) {
      return [
        FilledButton.icon(
          onPressed: enabled ? _fund : null,
          icon: const Icon(Icons.payments_outlined, size: 18),
          label: const Text('Fund'),
        ),
      ];
    }
    switch (e.status) {
      case 'created':
        return [
          Text(
            'Escrow created — funds committed. Waiting for fulfillment.',
            style: Theme.of(context).textTheme.bodySmall,
          ),
          const SizedBox(height: 6),
          TextButton.icon(
            onPressed: enabled ? () => _disputeDialog(e) : null,
            icon: const Icon(Icons.warning_amber, size: 18),
            label: const Text('Dispute'),
          ),
        ];
      case 'disputed':
        return [
          Wrap(
            spacing: 8,
            runSpacing: 8,
            children: [
              FilledButton.icon(
                onPressed: enabled ? () => _release(e) : null,
                icon: const Icon(Icons.check, size: 18),
                label: const Text('Release'),
              ),
              OutlinedButton.icon(
                onPressed: enabled ? () => _resolveDialog(e) : null,
                icon: const Icon(Icons.gavel, size: 18),
                label: const Text('Resolve'),
              ),
              TextButton.icon(
                onPressed: enabled ? () => _disputeDialog(e) : null,
                icon: const Icon(Icons.warning_amber, size: 18),
                label: const Text('Dispute'),
              ),
            ],
          ),
        ];
      case 'completed':
        return [
          const Text('✓ Escrow completed',
              style: TextStyle(color: Color(0xFF16a34a))),
        ];
      case 'refunded':
        return [
          Text('↩ Refunded', style: Theme.of(context).textTheme.bodySmall),
        ];
      default:
        return [
          Text('Status: ${e.status}',
              style: Theme.of(context).textTheme.bodySmall),
        ];
    }
  }

  Widget _statusChip(String status) {
    final (color, label) = switch (status) {
      'created' => (const Color(0xFF4f8cff), 'Created'),
      'disputed' => (const Color(0xFFd97706), 'Dispute in review'),
      'completed' => (const Color(0xFF16a34a), 'Completed'),
      'refunded' => (const Color(0xFF6b7280), 'Refunded'),
      _ => (const Color(0xFF6b7280), status.isEmpty ? 'unknown' : status),
    };
    return Container(
      padding: const EdgeInsets.symmetric(horizontal: 10, vertical: 3),
      decoration: BoxDecoration(
        color: color,
        borderRadius: BorderRadius.circular(999),
      ),
      child: Text(
        'Status: $label',
        style: const TextStyle(
          color: Colors.white,
          fontSize: 12,
          fontWeight: FontWeight.w600,
        ),
      ),
    );
  }

  Widget _roleBadge(String label) {
    return Container(
      padding: const EdgeInsets.symmetric(horizontal: 10, vertical: 3),
      decoration: BoxDecoration(
        border: Border.all(color: Theme.of(context).colorScheme.outline),
        borderRadius: BorderRadius.circular(999),
      ),
      child: Text(label, style: const TextStyle(fontSize: 12)),
    );
  }
}
