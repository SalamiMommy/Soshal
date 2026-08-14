import 'package:flutter_test/flutter_test.dart';
import './helpers/test_env.dart';

import 'package:soshal_flutter/ffi/marketplace.dart';

void main() {
  test('marketplace wrappers call api', () {
    final tmp = '/tmp/soshal-test-${DateTime.now().microsecondsSinceEpoch}';
    final env = bootstrapTestEnv(tmp);
    final api = env.$1 as FakeApi;

    api.stubString('crateFfiMarketplaceMarketplaceFetchListings', '[]');
    api.stubString('crateFfiMarketplaceMarketplaceGetListing', '{}');
    api.stubBool('crateFfiMarketplaceMarketplaceUpdateListing', true);
    api.stub('crateFfiMarketplaceMarketplaceListingRating', (_) => 4.5);

    final l = marketplaceFetchListings(limit: 10, offset: 0);
    expect(l, '[]');

    final item = marketplaceGetListing(listingId: 'id');
    expect(item, '{}');

    final upd = marketplaceUpdateListing(listingId: 'id', sellerPubkey: 's', title: 't', description: 'd', price: BigInt.from(1));
    expect(upd, true);

    final rating = marketplaceListingRating(listingId: 'id');
    expect(rating, 4.5);

    expect(api.callCount('crateFfiMarketplaceMarketplaceFetchListings'), 1);
  });
}
