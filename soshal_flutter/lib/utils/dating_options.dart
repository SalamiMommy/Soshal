/// Canonical dating-profile option lists, shared by the edit form
/// (dating_profile_screen) and the discovery filter dialog (dating_screen).
///
/// Each list keeps the `''` sentinel first so the two screens can render it
/// as "Not set" (form) or "Any" (filter) at the call site — no string is
/// baked in here, so the lists themselves can never drift apart.
library;

const kGenderOptions = ['', 'male', 'female', 'non-binary', 'other'];
const kSeekingOptions = ['', 'male', 'female', 'non-binary', 'other', 'All'];
const kBodyTypeOptions = [
  '',
  'slim',
  'athletic',
  'average',
  'curvy',
  'muscular',
];
const kSmokingOptions = ['', 'never', 'occasionally', 'regularly'];
const kDrinkingOptions = ['', 'never', 'socially', 'regularly'];
const kIntentOptions = ['', 'serious', 'casual', 'still figuring out'];
const kPoliticsOptions = [
  '',
  'prefer not to say',
  'liberal',
  'moderate',
  'conservative',
  'libertarian',
  'other',
];
const kEducationOptions = [
  '',
  'high school',
  'some college',
  'associate',
  'trade school',
  "bachelor's",
  "master's",
  'doctorate',
];