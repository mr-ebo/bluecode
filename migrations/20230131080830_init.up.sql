CREATE TYPE Status AS ENUM ('Processing', 'Approved', 'Declined', 'Failed');

CREATE TABLE payments (
    id uuid PRIMARY KEY,
    amount integer NOT NULL,
    card_number character varying(255) NOT NULL,
    status Status  NOT NULL,
    inserted_at timestamp(0) without time zone NOT NULL,
    updated_at timestamp(0) without time zone NOT NULL
);
-- CREATE UNIQUE INDEX payments_pkey ON payments(id uuid_ops);
CREATE UNIQUE INDEX payments_id_index ON payments(id uuid_ops);
CREATE UNIQUE INDEX payments_card_number_index ON payments(card_number text_ops);

CREATE TABLE refunds (
    id uuid PRIMARY KEY,
    payment_id uuid REFERENCES payments(id) NOT NULL,
    amount integer NOT NULL,
    inserted_at timestamp(0) without time zone NOT NULL,
    updated_at timestamp(0) without time zone NOT NULL
);

-- CREATE UNIQUE INDEX refunds_pkey ON refunds(id uuid_ops);
CREATE UNIQUE INDEX refunds_id_index ON refunds(id uuid_ops);
CREATE INDEX refunds_payment_id_index ON refunds(payment_id uuid_ops);
