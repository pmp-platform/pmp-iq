-- migrate:up
CREATE TABLE entity_kinds (
    id          UUID PRIMARY KEY,
    entity_type TEXT NOT NULL,
    value       TEXT NOT NULL,
    UNIQUE (entity_type, value)
);

CREATE TABLE entity_properties (
    id          UUID PRIMARY KEY,
    entity_type TEXT NOT NULL,
    prop_id     TEXT NOT NULL,
    name        TEXT NOT NULL,
    data_type   TEXT NOT NULL,
    UNIQUE (entity_type, prop_id)
);

-- Bring libraries/users/groups into the metadata-bearing shape (apps + linked
-- entities already have it).
ALTER TABLE libraries ADD COLUMN metadata JSONB NOT NULL DEFAULT '{}';
ALTER TABLE users     ADD COLUMN metadata JSONB NOT NULL DEFAULT '{}';
ALTER TABLE groups    ADD COLUMN metadata JSONB NOT NULL DEFAULT '{}';

-- Seed well-known kinds (every list includes 'other', the coercion target).
INSERT INTO entity_kinds (id, entity_type, value) VALUES
  (gen_random_uuid(),'applications','api'),
  (gen_random_uuid(),'applications','frontend'),
  (gen_random_uuid(),'applications','mobile'),
  (gen_random_uuid(),'applications','cli'),
  (gen_random_uuid(),'applications','library'),
  (gen_random_uuid(),'applications','service'),
  (gen_random_uuid(),'applications','worker'),
  (gen_random_uuid(),'applications','other'),
  (gen_random_uuid(),'libraries','cargo'),
  (gen_random_uuid(),'libraries','npm'),
  (gen_random_uuid(),'libraries','pypi'),
  (gen_random_uuid(),'libraries','maven'),
  (gen_random_uuid(),'libraries','go'),
  (gen_random_uuid(),'libraries','gem'),
  (gen_random_uuid(),'libraries','composer'),
  (gen_random_uuid(),'libraries','nuget'),
  (gen_random_uuid(),'libraries','other'),
  (gen_random_uuid(),'infrastructure','database'),
  (gen_random_uuid(),'infrastructure','cache'),
  (gen_random_uuid(),'infrastructure','queue'),
  (gen_random_uuid(),'infrastructure','storage'),
  (gen_random_uuid(),'infrastructure','message-broker'),
  (gen_random_uuid(),'infrastructure','search'),
  (gen_random_uuid(),'infrastructure','other'),
  (gen_random_uuid(),'tools','build'),
  (gen_random_uuid(),'tools','orchestration'),
  (gen_random_uuid(),'tools','ci'),
  (gen_random_uuid(),'tools','iac'),
  (gen_random_uuid(),'tools','package-manager'),
  (gen_random_uuid(),'tools','testing'),
  (gen_random_uuid(),'tools','other'),
  (gen_random_uuid(),'cloud-providers','compute'),
  (gen_random_uuid(),'cloud-providers','storage'),
  (gen_random_uuid(),'cloud-providers','network'),
  (gen_random_uuid(),'cloud-providers','database'),
  (gen_random_uuid(),'cloud-providers','serverless'),
  (gen_random_uuid(),'cloud-providers','other'),
  (gen_random_uuid(),'services','payments'),
  (gen_random_uuid(),'services','auth'),
  (gen_random_uuid(),'services','email'),
  (gen_random_uuid(),'services','sms'),
  (gen_random_uuid(),'services','messaging'),
  (gen_random_uuid(),'services','api'),
  (gen_random_uuid(),'services','other'),
  (gen_random_uuid(),'platforms','observability'),
  (gen_random_uuid(),'platforms','identity'),
  (gen_random_uuid(),'platforms','ci'),
  (gen_random_uuid(),'platforms','error-tracking'),
  (gen_random_uuid(),'platforms','version-control'),
  (gen_random_uuid(),'platforms','analytics'),
  (gen_random_uuid(),'platforms','other'),
  (gen_random_uuid(),'external','api'),
  (gen_random_uuid(),'external','webhook'),
  (gen_random_uuid(),'external','cdn'),
  (gen_random_uuid(),'external','dataset'),
  (gen_random_uuid(),'external','other');

-- Seed a standard property set per entity type.
INSERT INTO entity_properties (id, entity_type, prop_id, name, data_type) VALUES
  (gen_random_uuid(),'applications','language_version','Language Version','string'),
  (gen_random_uuid(),'applications','framework','Framework','string'),
  (gen_random_uuid(),'applications','repository_url','Repository URL','string'),
  (gen_random_uuid(),'applications','team','Team','string'),
  (gen_random_uuid(),'applications','deployment_target','Deployment Target','string'),
  (gen_random_uuid(),'libraries','license','License','string'),
  (gen_random_uuid(),'libraries','purpose','Purpose','string'),
  (gen_random_uuid(),'infrastructure','provider','Provider','string'),
  (gen_random_uuid(),'infrastructure','region','Region','string'),
  (gen_random_uuid(),'tools','config_file','Config File','string'),
  (gen_random_uuid(),'cloud-providers','region','Region','string'),
  (gen_random_uuid(),'cloud-providers','services_used','Services Used','array_of_strings'),
  (gen_random_uuid(),'services','provider','Provider','string'),
  (gen_random_uuid(),'services','auth_method','Auth Method','string'),
  (gen_random_uuid(),'services','base_url','Base URL','string'),
  (gen_random_uuid(),'platforms','plan_tier','Plan Tier','string'),
  (gen_random_uuid(),'platforms','region','Region','string'),
  (gen_random_uuid(),'external','url','URL','string'),
  (gen_random_uuid(),'users','full_name','Full Name','string'),
  (gen_random_uuid(),'users','role','Role','string'),
  (gen_random_uuid(),'groups','description','Description','string');

-- migrate:down
