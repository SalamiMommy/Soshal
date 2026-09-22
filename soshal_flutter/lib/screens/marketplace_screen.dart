import 'dart:convert';

import 'package:flutter/material.dart';
import 'package:go_router/go_router.dart';
import 'package:provider/provider.dart';
import '../services/friends_service.dart';
import '../services/marketplace_service.dart';
import '../services/media_service.dart';
import '../services/session_service.dart';
import '../utils/format.dart';
import '../utils/media_upload.dart';
import '../widgets/app_snack.dart';
import '../widgets/audience_filter_dropdown.dart';
import '../widgets/blob_image.dart';
import '../widgets/empty_state.dart';

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
  final TextEditingController _search = TextEditingController();
  Future<List<ListingInfo>>? _sellerListingsFuture;
  bool _trending = false;
  int _radiusKm = 25;
  AudienceFilter _audienceFilter = AudienceFilter.all;
  String _selectedCondition = 'All';
  final List<String> _conditions = const [
    'All',
    'New',
    'Like New',
    'Good',
    'Fair'
  ];

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
    try {
      final api = context.read<MarketplaceService>();
      final session = context.read<SessionService>();
      final q = _search.text.trim();
      if (session.activePubkey != null) {
        context.friendsServiceReadOrNull
            ?.loadAudienceGraph(session.activePubkey!);
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
  }

  Future<String?> _createDialog() async {
    final result = await showDialog<_CreateListingResult>(
      context: context,
      builder: (_) => const _CreateListingDialog(),
    );
    if (result == null || !result.ok || !mounted) return null;
    try {
      final session = context.read<SessionService>();
      final pubkey = session.activePubkey;
      if (pubkey == null) throw Exception('Sign in to sell');
      final eventId = await context.read<MarketplaceService>().createListing(
            pubkey,
            result.title,
            result.desc,
            result.price,
            result.currency.isEmpty ? 'sats' : result.currency,
            result.category,
            result.condition.isEmpty ? 'new' : result.condition,
            result.images,
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
    ListingInfo detail = listing;
    try {
      detail = await context.read<MarketplaceService>().getListing(listing.id);
    } catch (e) {
      debugPrint('listing detail: $e');
    }
    if (!mounted) return;
    await showDialog<void>(
      context: context,
      builder: (context) => AlertDialog(
        title: Text(detail.title),
        content: SingleChildScrollView(
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.start,
            mainAxisSize: MainAxisSize.min,
            children: [
              Text(detail.priceLabel,
                  style: Theme.of(context).textTheme.titleMedium),
              const SizedBox(height: 8),
              if (detail.description.isNotEmpty) ...[
                Text(detail.description),
                const SizedBox(height: 8),
              ],
              _ListingDetailMeta(listing: detail, myPubkey: myPubkey),
              const Divider(height: 20),
              _EscrowSection(listing: detail, myPubkey: myPubkey),
              const Divider(height: 20),
              _ReviewsSection(listing: detail, myPubkey: myPubkey),
              const Divider(height: 20),
              _PollSection(myPubkey: myPubkey),
            ],
          ),
        ),
        actions: [
          IconButton(
            icon: const Icon(Icons.bookmark_border),
            tooltip: 'Save item to Watchlist',
            onPressed: () {
              // No marketplace-core watchlist table exists yet.
              ScaffoldMessenger.of(context).showSnackBar(
                const SnackBar(
                    content: Text('Watchlist unavailable (roadmap)')),
              );
            },
          ),
          OutlinedButton.icon(
            icon: const Icon(Icons.chat_outlined, size: 16),
            label: const Text('Message'),
            onPressed: () {
              Navigator.pop(context);
              context.push('/inbox/${detail.sellerPubkey}');
            },
          ),
          OutlinedButton(
            onPressed: () {
              Navigator.pop(context);
              _makeOffer(detail);
            },
            child: const Text('Make offer'),
          ),
          FilledButton(
            onPressed: () {
              Navigator.pop(context);
              _buy(detail);
            },
            child: const Text('Buy'),
          ),
        ],
      ),
    );
  }

  Future<void> _makeOffer(ListingInfo listing) async {
    final result = await showDialog<_MakeOfferResult>(
      context: context,
      builder: (_) => _MakeOfferDialog(
        listingTitle: listing.title,
        listingCurrency: listing.currency,
        listedPrice: listing.price,
      ),
    );
    if (result == null || !result.ok || !mounted) return;
    // Offer events are NOT transported yet — no outbox-relayed offer kind
    // exists in marketplace-core, so claiming success would be a lie.
    ScaffoldMessenger.of(context).showSnackBar(
      const SnackBar(content: Text('Offers unavailable (roadmap)')),
    );
    debugPrint('marketplace offer requested: ${result.offerAmount} '
        '${listing.currency} for ${listing.title}');
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
        var escrowCount = '';
        try {
          final escrows = await api.escrowsByParticipant(pubkey);
          escrowCount = ' (${escrows.length} total)';
        } catch (e) {
          debugPrint('marketplace: $e');
        }
        if (mounted) {
          ScaffoldMessenger.of(context).showSnackBar(
            SnackBar(
              content: SelectableText('Escrow $escrowId created$escrowCount'),
            ),
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

  Future<void> _orderDetail(OrderInfo order) async {
    OrderInfo detail = order;
    try {
      detail = await context.read<MarketplaceService>().getOrder(order.id);
    } catch (e) {
      debugPrint('order detail: $e');
    }
    if (!mounted) return;
    await showDialog<void>(
      context: context,
      builder: (context) => AlertDialog(
        title: Text('Order ${firstChars(detail.id, 8)}'),
        content: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          mainAxisSize: MainAxisSize.min,
          children: [
            Text('Status: ${detail.status}',
                style: Theme.of(context).textTheme.titleMedium),
            const SizedBox(height: 8),
            Text('Amount: ${detail.amount} sats'),
            Text('Listing: ${firstChars(detail.listingId, 12)}'),
            Text('Buyer: ${shortPubkey(detail.buyerPubkey, head: 8, tail: 4)}'),
            Text(
                'Seller: ${shortPubkey(detail.sellerPubkey, head: 8, tail: 4)}'),
            if (detail.createdAt > 0)
              Text('Created: ${formatTimestamp(detail.createdAt)}'),
          ],
        ),
        actions: [
          TextButton(
            onPressed: () => Navigator.pop(context),
            child: const Text('Close'),
          ),
        ],
      ),
    );
  }

  Future<void> _editDialog(ListingInfo listing) async {
    final result = await showDialog<_EditListingResult>(
      context: context,
      builder: (_) => _EditListingDialog(
        title: listing.title,
        desc: listing.description,
        price: listing.price,
      ),
    );
    if (result == null || !result.ok || !mounted) return;
    try {
      final session = context.read<SessionService>();
      final pubkey = session.activePubkey;
      if (pubkey == null) throw Exception('Sign in to edit');
      final updated = await context.read<MarketplaceService>().updateListing(
            listing.id,
            pubkey,
            result.title.trim(),
            result.desc.trim(),
            int.tryParse(result.price.trim()) ?? listing.price,
          );
      if (mounted) {
        ScaffoldMessenger.of(context).showSnackBar(
          SnackBar(
            content: SelectableText(updated ? 'Listing updated' : 'No change'),
          ),
        );
      }
      await _load();
    } catch (e) {
      if (mounted) {
        ScaffoldMessenger.of(context).showSnackBar(
          SnackBar(content: SelectableText('Update failed: $e')),
        );
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
          AudienceFilterDropdown(
            value: _audienceFilter,
            onChanged: (val) => setState(() => _audienceFilter = val),
          ),
          IconButton(
            icon: const Icon(Icons.tune),
            tooltip: 'Filter by radius and condition',
            onPressed: () {
              showModalBottomSheet<void>(
                context: context,
                builder: (sheetContext) => StatefulBuilder(
                  builder: (sheetContext, setSheetState) => SafeArea(
                    child: Padding(
                      padding: const EdgeInsets.all(20),
                      child: Column(
                        mainAxisSize: MainAxisSize.min,
                        crossAxisAlignment: CrossAxisAlignment.start,
                        children: [
                          Text('Marketplace Filters',
                              style: Theme.of(context).textTheme.titleLarge),
                          const SizedBox(height: 16),
                          Text('Search Radius: $_radiusKm km',
                              style:
                                  const TextStyle(fontWeight: FontWeight.bold)),
                          Slider(
                            value: _radiusKm.toDouble(),
                            min: 5,
                            max: 100,
                            divisions: 19,
                            label: '$_radiusKm km',
                            onChanged: (v) {
                              setSheetState(() => _radiusKm = v.round());
                              setState(() => _radiusKm = v.round());
                            },
                          ),
                          const SizedBox(height: 8),
                          const Text('Item Condition',
                              style: TextStyle(fontWeight: FontWeight.bold)),
                          const SizedBox(height: 8),
                          Wrap(
                            spacing: 8,
                            children: [
                              for (final cond in _conditions)
                                ChoiceChip(
                                  label: Text(cond),
                                  selected: _selectedCondition == cond,
                                  onSelected: (selected) {
                                    if (selected) {
                                      setSheetState(
                                          () => _selectedCondition = cond);
                                      setState(() => _selectedCondition = cond);
                                    }
                                  },
                                ),
                            ],
                          ),
                          const SizedBox(height: 16),
                          SizedBox(
                            width: double.infinity,
                            child: FilledButton(
                              onPressed: () => Navigator.pop(sheetContext),
                              child: const Text('Apply Filters'),
                            ),
                          ),
                        ],
                      ),
                    ),
                  ),
                ),
              );
            },
          ),
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
      body: Column(
        children: [
          // Filter strip stays visible while a category/trending load runs —
          // only the listing region below shows the spinner.
          SizedBox(
            height: 40,
            child: ListView(
              scrollDirection: Axis.horizontal,
              padding: const EdgeInsets.symmetric(horizontal: 8),
              children: [
                Padding(
                  padding: const EdgeInsets.symmetric(horizontal: 4),
                  child: ChoiceChip(
                    label: const Text('🔥 Trending'),
                    selected: _trending,
                    onSelected: (_) async {
                      _trending = true;
                      _category = '';
                      setState(() {});
                      try {
                        await context.read<MarketplaceService>().trending();
                      } catch (e) {
                        debugPrint('trending: $e');
                      }
                    },
                  ),
                ),
                for (final c in _categories)
                  Padding(
                    padding: const EdgeInsets.symmetric(horizontal: 4),
                    child: ChoiceChip(
                      label: Text(c.isEmpty ? 'All' : c),
                      selected: _category == c && !_trending,
                      onSelected: (_) async {
                        _category = c;
                        _trending = false;
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
            child: context.select((MarketplaceService s) => s.listingsLoading)
                ? const Center(child: CircularProgressIndicator())
                : TabBarView(
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
    'vehicles',
    'property rentals',
    'electronics',
    'apparel & accessories',
    'home & garden',
    'sporting goods',
    'toys & games',
    'pet supplies',
    'free stuff',
    'handmade',
    'art',
    'books',
  ];

  String _category = '';

  Widget _buildBrowse() {
    return Consumer<MarketplaceService>(
      builder: (context, api, _) {
        final rawListings = api.listings;
        final friendsService = context.friendsServiceOrNull;
        final myPk =
            context.select<SessionService, String?>((s) => s.activePubkey);
        final audienceListings = friendsService != null
            ? friendsService.filterList(
                rawListings,
                _audienceFilter,
                (l) => l.sellerPubkey,
                myPubkey: myPk,
              )
            : rawListings;
        final listings = _selectedCondition == 'All'
            ? audienceListings
            : audienceListings
                .where((l) =>
                    l.condition.toLowerCase() ==
                    _selectedCondition.toLowerCase())
                .toList();
        if (listings.isEmpty) {
          return const EmptyState(
            icon: Icons.storefront_outlined,
            title: 'No listings match filter',
          );
        }
        return RefreshIndicator(
          onRefresh: _load,
          child: ListView.builder(
            itemExtent: 96.0,
            itemCount: listings.length,
            itemBuilder: (context, index) {
              final l = listings[index];
              return Card(
                margin: const EdgeInsets.symmetric(horizontal: 12, vertical: 6),
                child: ListTile(
                  leading: l.images.isNotEmpty
                      ? ClipRRect(
                          borderRadius: BorderRadius.circular(8),
                          child: BlobImage(
                            source: l.images.first,
                            width: 56,
                            height: 56,
                            errorBuilder: (_) => const SizedBox(
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
                        '${l.priceLabel} · ${l.sellerName.isEmpty ? firstChars(l.sellerPubkey, 8) : l.sellerName}'
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
          return const EmptyState(
            icon: Icons.receipt_long_outlined,
            title: 'No orders yet',
          );
        }
        return ListView.builder(
          itemExtent: 64.0,
          itemCount: api.orders.length,
          itemBuilder: (context, index) {
            final o = api.orders[index];
            return ListTile(
              leading: const Icon(Icons.receipt_long),
              title: Text('Order ${firstChars(o.id, 8)}'),
              subtitle: Text('${o.amount} sats · ${o.status}'),
              onTap: () => _orderDetail(o),
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
          return const EmptyState(
            icon: Icons.storefront_outlined,
            title: 'Sign in to see your listings',
          );
        }
        return FutureBuilder<List<ListingInfo>>(
          future: _sellerListingsFuture,
          builder: (context, snapshot) {
            final mine = snapshot.data ?? [];
            if (mine.isEmpty) {
              return const EmptyState(
                icon: Icons.storefront_outlined,
                title: 'You have no listings',
              );
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
                  trailing: Row(
                    mainAxisSize: MainAxisSize.min,
                    children: [
                      IconButton(
                        icon: const Icon(Icons.edit_outlined),
                        tooltip: 'Edit',
                        onPressed: () => _editDialog(l),
                      ),
                      IconButton(
                        icon: const Icon(Icons.delete_outline),
                        tooltip: 'Delete',
                        onPressed: () async {
                          try {
                            await api.deleteListing(l.id, pubkey);
                            if (!context.mounted) return;
                            _sellerListingsFuture = api.sellerListings(pubkey);
                            setState(() {});
                          } catch (e) {
                            if (context.mounted) {
                              ScaffoldMessenger.of(context).showSnackBar(
                                SnackBar(
                                    content:
                                        SelectableText('Delete failed: $e')),
                              );
                            }
                          }
                        },
                      ),
                    ],
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

class _CreateListingResult {
  const _CreateListingResult({
    required this.ok,
    this.title = '',
    this.desc = '',
    this.price = 0,
    this.currency = 'sats',
    this.category = '',
    this.condition = 'new',
    this.images = const [],
  });

  final bool ok;
  final String title;
  final String desc;
  final int price;
  final String currency;
  final String category;
  final String condition;
  final List<String> images;
}

class _CreateListingDialog extends StatefulWidget {
  const _CreateListingDialog();

  @override
  State<_CreateListingDialog> createState() => _CreateListingDialogState();
}

class _CreateListingDialogState extends State<_CreateListingDialog> {
  final _title = TextEditingController();
  final _desc = TextEditingController();
  final _price = TextEditingController();
  final _currency = TextEditingController(text: 'sats');
  final _category = TextEditingController();
  final _condition = TextEditingController(text: 'new');
  final _images = TextEditingController();

  @override
  void dispose() {
    _title.dispose();
    _desc.dispose();
    _price.dispose();
    _currency.dispose();
    _category.dispose();
    _condition.dispose();
    _images.dispose();
    super.dispose();
  }

  List<String> _imagesList() {
    final parsed = _images.text.trim();
    if (parsed.isEmpty) return const [];
    try {
      return (jsonDecode(parsed) as List<dynamic>)
          .map((e) => e.toString())
          .toList();
    } catch (_) {
      return parsed
          .split(',')
          .map((e) => e.trim())
          .where((e) => e.isNotEmpty)
          .toList();
    }
  }

  Future<void> _pickImage() async {
    try {
      final blob = await pickAndUploadMedia(
        (p) => context.read<MediaService>().uploadMedia(p),
        errorMessage: 'Bad upload manifest',
      );
      if (blob == null || !mounted) return;
      final list = [..._imagesList(), blob.uri];
      setState(() => _images.text = jsonEncode(list));
    } catch (e) {
      if (mounted) {
        ScaffoldMessenger.of(context).showSnackBar(
            SnackBar(content: SelectableText('Upload error: $e')));
      }
    }
  }

  void _pop(bool ok) {
    Navigator.of(context).pop(_CreateListingResult(
      ok: ok,
      title: _title.text.trim(),
      desc: _desc.text.trim(),
      price: int.tryParse(_price.text.trim()) ?? 0,
      currency: _currency.text.trim(),
      category: _category.text.trim(),
      condition: _condition.text.trim(),
      images: _imagesList(),
    ));
  }

  @override
  Widget build(BuildContext context) {
    return AlertDialog(
      title: const Text('Create listing'),
      content: SingleChildScrollView(
        child: Column(
          mainAxisSize: MainAxisSize.min,
          children: [
            TextField(
              controller: _title,
              decoration: const InputDecoration(labelText: 'Title *'),
            ),
            TextField(
              controller: _desc,
              maxLines: 3,
              decoration: const InputDecoration(labelText: 'Description'),
            ),
            TextField(
              controller: _price,
              keyboardType: TextInputType.number,
              decoration: const InputDecoration(labelText: 'Price *'),
            ),
            TextField(
              controller: _currency,
              decoration: const InputDecoration(labelText: 'Currency'),
            ),
            TextField(
              controller: _category,
              decoration: const InputDecoration(labelText: 'Category'),
            ),
            TextField(
              controller: _condition,
              decoration: const InputDecoration(labelText: 'Condition'),
            ),
            TextField(
              controller: _images,
              decoration: const InputDecoration(
                labelText: 'Images',
                hintText: 'pick from device or paste URLs',
              ),
            ),
            const SizedBox(height: 8),
            OutlinedButton.icon(
              onPressed: _pickImage,
              icon: const Icon(Icons.add_photo_alternate_outlined),
              label: const Text('Pick image from device'),
            ),
          ],
        ),
      ),
      actions: [
        TextButton(
          onPressed: () => _pop(false),
          child: const Text('Cancel'),
        ),
        FilledButton(
          onPressed: () => _pop(true),
          child: const Text('Post'),
        ),
      ],
    );
  }
}

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
                'Seller: ${listing.sellerName.isEmpty ? shortPubkey(seller, head: 8, tail: 4) : listing.sellerName}'
                ' · ${shortPubkey(seller, head: 8, tail: 4)}',
                style: Theme.of(context).textTheme.bodySmall,
                maxLines: 1,
                overflow: TextOverflow.ellipsis,
              ),
            ),
            if (seller.isNotEmpty && seller != widget.myPubkey)
              FilledButton.tonal(
                onPressed: () => context.push('/inbox/$seller'),
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
    if (escrow != null) {
      try {
        final full = await _api.getEscrow(escrow.id);
        if (!mounted) return;
        setState(() => _escrow = full);
        return;
      } catch (e) {
        debugPrint('escrow detail: $e');
      }
    }
    setState(() => _escrow = escrow);
  }

  void _snack(String message) {
    if (!mounted) return;
    showAppSnack(context, message);
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
      await _refresh();
      if (!mounted) return;
      setState(() => _busy = false);
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
    } catch (e) {
      debugPrint('marketplace: $e');
    }
    return null;
  }

  Future<void> _fund() async {
    final order = await _findOrder(widget.listing.id);
    if (order == null) {
      _snack('An order is required to fund escrow — tap "Buy" on the '
          'listing to create one first.');
      return;
    }
    await _run('Escrow funded — ${shortPubkey(order.id, head: 8, tail: 4)}',
        () async {
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

  Future<void> _confirm(EscrowInfo escrow, bool isBuyer) => _run(
        isBuyer ? 'Buyer confirmed ✓' : 'Seller confirmed ✓',
        () => isBuyer
            ? _api.confirmEscrowBuyer(escrow.id, escrow.buyerPubkey)
            : _api.confirmEscrowSeller(escrow.id, escrow.sellerPubkey),
      );

  Future<void> _disputeDialog(EscrowInfo escrow) async {
    final result = await showDialog<_DisputeResult>(
      context: context,
      builder: (_) => const _DisputeDialog(),
    );
    if (result == null || !result.ok || !mounted) return;
    await _run('Dispute opened — mediator notified ⚠️', () async {
      await _api.disputeEscrow(
          escrow.id, widget.myPubkey, result.reason.trim());
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
            child: Text(
                'Resolve for buyer (${shortPubkey(escrow.buyerPubkey, head: 8, tail: 4)})'),
          ),
          SimpleDialogOption(
            onPressed: () => Navigator.pop(context, escrow.sellerPubkey),
            child: Text(
                'Resolve for seller (${shortPubkey(escrow.sellerPubkey, head: 8, tail: 4)})'),
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
              color: Theme.of(context).colorScheme.surfaceContainerHighest,
              border: Border.all(color: Theme.of(context).colorScheme.outline),
              borderRadius: BorderRadius.circular(999),
            ),
            child: Row(
              mainAxisSize: MainAxisSize.min,
              children: [
                const Text('🔒 ', style: TextStyle(fontSize: 12)),
                Text(
                  'Escrow',
                  style: TextStyle(
                    fontSize: 12,
                    fontWeight: FontWeight.w600,
                    color: Theme.of(context).colorScheme.onSurfaceVariant,
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
          Wrap(
            spacing: 8,
            runSpacing: 8,
            children: [
              if (e.buyerPubkey == widget.myPubkey)
                OutlinedButton.icon(
                  onPressed: enabled ? () => _confirm(e, true) : null,
                  icon: const Icon(Icons.verified_outlined, size: 18),
                  label: const Text('Confirm as buyer'),
                ),
              if (e.sellerPubkey == widget.myPubkey)
                OutlinedButton.icon(
                  onPressed: enabled ? () => _confirm(e, false) : null,
                  icon: const Icon(Icons.verified_outlined, size: 18),
                  label: const Text('Confirm as seller'),
                ),
              TextButton.icon(
                onPressed: enabled ? () => _disputeDialog(e) : null,
                icon: const Icon(Icons.warning_amber, size: 18),
                label: const Text('Dispute'),
              ),
            ],
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

/// Listing reviews: average rating + review list + "write review" dialog.
class _ReviewsSection extends StatefulWidget {
  const _ReviewsSection({required this.listing, required this.myPubkey});

  final ListingInfo listing;
  final String myPubkey;

  @override
  State<_ReviewsSection> createState() => _ReviewsSectionState();
}

class _ReviewsSectionState extends State<_ReviewsSection> {
  double _rating = 0;
  List<dynamic> _reviews = const [];
  bool _loaded = false;

  @override
  void initState() {
    super.initState();
    _load();
  }

  Future<void> _load() async {
    final api = context.read<MarketplaceService>();
    final rating = await api.listingRating(widget.listing.id);
    final reviews = await api.listingReviews(widget.listing.id);
    if (!mounted) return;
    setState(() {
      _rating = rating;
      _reviews = reviews;
      _loaded = true;
    });
  }

  Future<void> _reviewDialog() async {
    final result = await showDialog<_PostTextResult>(
      context: context,
      builder: (_) => const _PostCommentDialog(),
    );
    if (result == null || !result.ok || !mounted) return;
    final submitted = await context.read<MarketplaceService>().reviewListing(
          listingId: widget.listing.id,
          reviewerPubkey: widget.myPubkey,
          rating: result.stars,
          text: result.text.trim(),
        );
    if (mounted) {
      ScaffoldMessenger.of(context).showSnackBar(
        SnackBar(
          content:
              SelectableText(submitted ? 'Review submitted' : 'Review failed'),
        ),
      );
    }
    await _load();
  }

  @override
  Widget build(BuildContext context) {
    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      mainAxisSize: MainAxisSize.min,
      children: [
        Row(
          children: [
            const Icon(Icons.star, color: Color(0xFFd97706), size: 18),
            const SizedBox(width: 4),
            Text(
              _loaded
                  ? '${_rating.toStringAsFixed(1)} · ${_reviews.length} reviews'
                  : 'Reviews…',
              style: Theme.of(context).textTheme.titleSmall,
            ),
            const Spacer(),
            if (widget.myPubkey.isNotEmpty)
              FilledButton.tonal(
                onPressed: _reviewDialog,
                child: const Text('Review'),
              ),
          ],
        ),
        const SizedBox(height: 6),
        if (_loaded && _reviews.isEmpty)
          Text('No reviews yet — be the first!',
              style: Theme.of(context).textTheme.bodySmall)
        else
          for (final r in _reviews)
            if (r is Map<String, dynamic>)
              Padding(
                padding: const EdgeInsets.only(bottom: 6),
                child: Column(
                  crossAxisAlignment: CrossAxisAlignment.start,
                  children: [
                    Row(
                      children: [
                        Icon(Icons.star,
                            color: const Color(0xFFd97706),
                            size: 14,
                            semanticLabel: '${r['rating']} stars'),
                        const SizedBox(width: 4),
                        Text(
                          '${r['rating'] ?? '?'} · ${shortPubkey(r['reviewer'] as String? ?? '', head: 8, tail: 4)}',
                          style: Theme.of(context).textTheme.bodySmall,
                        ),
                      ],
                    ),
                    if ((r['text'] as String? ?? '').isNotEmpty)
                      Text(r['text'] as String,
                          style: Theme.of(context).textTheme.bodySmall),
                  ],
                ),
              ),
      ],
    );
  }
}

/// Marketplace polls: create dialog, vote, close and status display.
class _PollSection extends StatefulWidget {
  const _PollSection({required this.myPubkey});

  final String myPubkey;

  @override
  State<_PollSection> createState() => _PollSectionState();
}

class _PollSectionState extends State<_PollSection> {
  Map<String, dynamic>? _poll;
  List<String> _options = const [];
  bool _hasVoted = false;
  bool _busy = false;
  final TextEditingController _pollId = TextEditingController();

  MarketplaceService get _api => context.read<MarketplaceService>();

  @override
  void dispose() {
    _pollId.dispose();
    super.dispose();
  }

  void _snack(String message) {
    if (!mounted) return;
    showAppSnack(context, message);
  }

  Future<void> _loadPoll(String pollId) async {
    final poll = await _api.pollGet(pollId);
    if (!mounted) return;
    setState(() {
      _poll = poll;
      _options = const [];
    });
    if (poll != null && widget.myPubkey.isNotEmpty) {
      final voted = await _api.pollHasVoted(pollId, widget.myPubkey);
      if (mounted) setState(() => _hasVoted = voted);
    }
  }

  Future<void> _createDialog() async {
    if (widget.myPubkey.isEmpty) {
      _snack('Sign in to create a poll');
      return;
    }
    final result = await showDialog<_PollResult>(
      context: context,
      builder: (_) => const _PollDialog(),
    );
    if (result == null || !result.ok || !mounted) return;
    final optionList = result.options
        .split(',')
        .map((e) => e.trim())
        .where((e) => e.isNotEmpty)
        .toList();
    if (optionList.length < 2) {
      _snack('A poll needs at least 2 options');
      return;
    }
    setState(() => _busy = true);
    final poll = await _api.pollCreate(
      userPubkey: widget.myPubkey,
      question: result.question,
      optionsJson: jsonEncode(optionList),
    );
    if (!mounted) return;
    setState(() {
      _busy = false;
      _poll = poll;
      _options = optionList;
      _hasVoted = false;
    });
    if (poll != null) {
      _snack('Poll created');
    } else {
      _snack('Poll creation failed');
    }
  }

  Future<void> _vote(int optionIndex) async {
    final id = _poll?['id'] as String?;
    if (id == null || widget.myPubkey.isEmpty) return;
    final ok = await _api.pollVote(
      pollId: id,
      voterPubkey: widget.myPubkey,
      optionIndex: optionIndex,
    );
    if (!mounted) return;
    _snack(ok ? 'Vote recorded' : 'Vote failed');
    if (ok) {
      setState(() => _hasVoted = true);
      await _loadPoll(id);
    }
  }

  Future<void> _closePoll() async {
    final id = _poll?['id'] as String?;
    if (id == null || widget.myPubkey.isEmpty) return;
    final ok = await _api.pollClose(id, widget.myPubkey);
    if (!mounted) return;
    _snack(ok ? 'Poll closed' : 'Close failed (owner only)');
    if (ok) await _loadPoll(id);
  }

  @override
  Widget build(BuildContext context) {
    final poll = _poll;
    final votes = (poll?['votes'] as List<dynamic>? ?? const []);
    final labels = _options.isNotEmpty
        ? _options
        : [for (var i = 0; i < votes.length; i++) 'Option ${i + 1}'];
    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      mainAxisSize: MainAxisSize.min,
      children: [
        Row(
          children: [
            const Icon(Icons.how_to_vote, size: 18),
            const SizedBox(width: 6),
            Text('Poll', style: Theme.of(context).textTheme.titleSmall),
            const Spacer(),
            if (widget.myPubkey.isNotEmpty)
              FilledButton.tonal(
                onPressed: _busy ? null : _createDialog,
                child: const Text('Create'),
              ),
          ],
        ),
        const SizedBox(height: 6),
        Row(
          children: [
            Expanded(
              child: TextField(
                controller: _pollId,
                decoration: const InputDecoration(
                  hintText: 'Poll ID',
                  isDense: true,
                ),
                onSubmitted: (id) {
                  if (id.trim().isNotEmpty) _loadPoll(id.trim());
                },
              ),
            ),
            IconButton(
              icon: const Icon(Icons.search),
              tooltip: 'Load poll',
              onPressed: () {
                final id = _pollId.text.trim();
                if (id.isNotEmpty) _loadPoll(id);
              },
            ),
          ],
        ),
        if (poll != null) ...[
          Text(poll['question'] as String? ?? '',
              style: Theme.of(context).textTheme.bodyMedium),
          const SizedBox(height: 4),
          for (var i = 0; i < labels.length; i++)
            Padding(
              padding: const EdgeInsets.only(bottom: 4),
              child: Row(
                children: [
                  Expanded(
                    child: Text(
                      '${labels[i]} — ${votes.length > i ? votes[i] : 0}',
                      style: Theme.of(context).textTheme.bodySmall,
                    ),
                  ),
                  if (!_hasVoted)
                    TextButton(
                      onPressed: () => _vote(i),
                      child: const Text('Vote'),
                    ),
                ],
              ),
            ),
          if (_hasVoted)
            Text('✓ You voted', style: Theme.of(context).textTheme.bodySmall),
          TextButton.icon(
            onPressed: widget.myPubkey.isEmpty ? null : _closePoll,
            icon: const Icon(Icons.lock_outline, size: 16),
            label: const Text('Close poll'),
          ),
        ] else
          Text('No poll loaded — create one or enter a poll ID above.',
              style: Theme.of(context).textTheme.bodySmall),
        if (_busy)
          const Padding(
            padding: EdgeInsets.only(top: 6),
            child: LinearProgressIndicator(),
          ),
      ],
    );
  }
}

class _MakeOfferResult {
  const _MakeOfferResult({
    required this.ok,
    required this.offerAmount,
    required this.note,
  });

  final bool ok;
  final String offerAmount;
  final String note;
}

class _MakeOfferDialog extends StatefulWidget {
  const _MakeOfferDialog({
    required this.listingTitle,
    required this.listingCurrency,
    required this.listedPrice,
  });

  final String listingTitle;
  final String listingCurrency;
  final int listedPrice;

  @override
  State<_MakeOfferDialog> createState() => _MakeOfferDialogState();
}

class _MakeOfferDialogState extends State<_MakeOfferDialog> {
  final _offerAmount = TextEditingController();
  final _note = TextEditingController();

  @override
  void dispose() {
    _offerAmount.dispose();
    _note.dispose();
    super.dispose();
  }

  void _pop(bool ok) {
    Navigator.of(context).pop(_MakeOfferResult(
      ok: ok,
      offerAmount: _offerAmount.text,
      note: _note.text,
    ));
  }

  @override
  Widget build(BuildContext context) {
    final currencyLabel =
        widget.listingCurrency.isEmpty ? 'sat' : widget.listingCurrency;
    return AlertDialog(
      title: Text('Make an offer on "${widget.listingTitle}"'),
      content: Column(
        mainAxisSize: MainAxisSize.min,
        children: [
          Text('Listed price: ${widget.listedPrice} $currencyLabel'),
          const SizedBox(height: 12),
          TextField(
            controller: _offerAmount,
            keyboardType: TextInputType.number,
            decoration: InputDecoration(
              labelText: 'Your offer (${widget.listingCurrency}) *',
              border: const OutlineInputBorder(),
            ),
          ),
          const SizedBox(height: 12),
          TextField(
            controller: _note,
            maxLines: 2,
            decoration: const InputDecoration(
              labelText: 'Note to seller (optional)',
              border: OutlineInputBorder(),
            ),
          ),
        ],
      ),
      actions: [
        TextButton(
          onPressed: () => _pop(false),
          child: const Text('Cancel'),
        ),
        FilledButton(
          onPressed: () => _pop(true),
          child: const Text('Send Offer'),
        ),
      ],
    );
  }
}

class _EditListingResult {
  const _EditListingResult({
    required this.ok,
    required this.title,
    required this.desc,
    required this.price,
  });

  final bool ok;
  final String title;
  final String desc;
  final String price;
}

class _EditListingDialog extends StatefulWidget {
  const _EditListingDialog({
    required this.title,
    required this.desc,
    required this.price,
  });

  final String title;
  final String desc;
  final int price;

  @override
  State<_EditListingDialog> createState() => _EditListingDialogState();
}

class _EditListingDialogState extends State<_EditListingDialog> {
  late final TextEditingController _title =
      TextEditingController(text: widget.title);
  late final TextEditingController _desc =
      TextEditingController(text: widget.desc);
  late final TextEditingController _price =
      TextEditingController(text: widget.price.toString());

  @override
  void dispose() {
    _title.dispose();
    _desc.dispose();
    _price.dispose();
    super.dispose();
  }

  void _pop(bool ok) {
    Navigator.of(context).pop(_EditListingResult(
      ok: ok,
      title: _title.text,
      desc: _desc.text,
      price: _price.text,
    ));
  }

  @override
  Widget build(BuildContext context) {
    return AlertDialog(
      title: const Text('Edit listing'),
      content: SingleChildScrollView(
        child: Column(
          mainAxisSize: MainAxisSize.min,
          children: [
            TextField(
              controller: _title,
              decoration: const InputDecoration(labelText: 'Title *'),
            ),
            TextField(
              controller: _desc,
              maxLines: 3,
              decoration: const InputDecoration(labelText: 'Description'),
            ),
            TextField(
              controller: _price,
              keyboardType: TextInputType.number,
              decoration: const InputDecoration(labelText: 'Price *'),
            ),
          ],
        ),
      ),
      actions: [
        TextButton(
          onPressed: () => _pop(false),
          child: const Text('Cancel'),
        ),
        FilledButton(
          onPressed: () => _pop(true),
          child: const Text('Save'),
        ),
      ],
    );
  }
}

class _DisputeResult {
  const _DisputeResult({
    required this.ok,
    required this.reason,
  });

  final bool ok;
  final String reason;
}

class _DisputeDialog extends StatefulWidget {
  const _DisputeDialog();

  @override
  State<_DisputeDialog> createState() => _DisputeDialogState();
}

class _DisputeDialogState extends State<_DisputeDialog> {
  final _reason = TextEditingController();

  @override
  void dispose() {
    _reason.dispose();
    super.dispose();
  }

  void _pop(bool ok) {
    Navigator.of(context).pop(_DisputeResult(
      ok: ok,
      reason: _reason.text,
    ));
  }

  @override
  Widget build(BuildContext context) {
    return AlertDialog(
      title: const Text('Open dispute'),
      content: TextField(
        controller: _reason,
        maxLines: 3,
        autofocus: true,
        decoration: const InputDecoration(hintText: 'Reason for dispute'),
      ),
      actions: [
        TextButton(
          onPressed: () => _pop(false),
          child: const Text('Cancel'),
        ),
        FilledButton(
          onPressed: () => _pop(true),
          child: const Text('Confirm dispute'),
        ),
      ],
    );
  }
}

class _PostTextResult {
  const _PostTextResult({
    required this.ok,
    required this.text,
    required this.stars,
  });

  final bool ok;
  final String text;
  final int stars;
}

class _PostCommentDialog extends StatefulWidget {
  const _PostCommentDialog();

  @override
  State<_PostCommentDialog> createState() => _PostCommentDialogState();
}

class _PostCommentDialogState extends State<_PostCommentDialog> {
  final _text = TextEditingController();
  int _stars = 5;

  @override
  void dispose() {
    _text.dispose();
    super.dispose();
  }

  void _pop(bool ok) {
    Navigator.of(context).pop(_PostTextResult(
      ok: ok,
      text: _text.text,
      stars: _stars,
    ));
  }

  @override
  Widget build(BuildContext context) {
    return AlertDialog(
      title: const Text('Review listing'),
      content: Column(
        mainAxisSize: MainAxisSize.min,
        children: [
          Wrap(
            spacing: 4,
            children: [
              for (var i = 1; i <= 5; i++)
                IconButton(
                  icon: Icon(
                    i <= _stars ? Icons.star : Icons.star_border,
                    color: const Color(0xFFd97706),
                  ),
                  onPressed: () => setState(() => _stars = i),
                ),
            ],
          ),
          TextField(
            controller: _text,
            maxLines: 3,
            decoration:
                const InputDecoration(hintText: 'Review text (optional)'),
          ),
        ],
      ),
      actions: [
        TextButton(
          onPressed: () => _pop(false),
          child: const Text('Cancel'),
        ),
        FilledButton(
          onPressed: () => _pop(true),
          child: const Text('Submit'),
        ),
      ],
    );
  }
}

class _PollResult {
  const _PollResult({required this.ok, this.question = '', this.options = ''});

  final bool ok;
  final String question;
  final String options;
}

class _PollDialog extends StatefulWidget {
  const _PollDialog();

  @override
  State<_PollDialog> createState() => _PollDialogState();
}

class _PollDialogState extends State<_PollDialog> {
  final _question = TextEditingController();
  final _options = TextEditingController();

  @override
  void dispose() {
    _question.dispose();
    _options.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    return AlertDialog(
      title: const Text('Create poll'),
      content: Column(
        mainAxisSize: MainAxisSize.min,
        children: [
          TextField(
            controller: _question,
            decoration: const InputDecoration(labelText: 'Question *'),
          ),
          TextField(
            controller: _options,
            decoration: const InputDecoration(
              labelText: 'Options * (comma-separated)',
            ),
          ),
        ],
      ),
      actions: [
        TextButton(
          onPressed: () => Navigator.of(context).pop(_PollResult(ok: false)),
          child: const Text('Cancel'),
        ),
        FilledButton(
          onPressed: () => Navigator.of(context).pop(_PollResult(
            ok: true,
            question: _question.text.trim(),
            options: _options.text.trim(),
          )),
          child: const Text('Post'),
        ),
      ],
    );
  }
}
